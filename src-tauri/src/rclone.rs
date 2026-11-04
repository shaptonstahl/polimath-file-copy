//! Transfer engine backed by rclone (MIT), driven through its remote-control
//! API (`rclone rcd`) on localhost.
//!
//! Today the paths are local (including synced folders and mapped drives).
//! Next step: let a path be an rclone remote such as `dropbox:Photos`, so
//! transfers can skip the desktop sync client and, between two paths on the
//! same provider, use server-side copy/move.

use crate::execute::Transfer;
use serde_json::{Value, json};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Find the rclone executable: `$RCLONE_PATH`, else `rclone` on `PATH`.
pub fn find() -> Option<PathBuf> {
    let candidate = std::env::var_os("RCLONE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rclone"));
    let ok = Command::new(&candidate)
        .arg("version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    ok.then_some(candidate)
}

pub fn version() -> Option<String> {
    let exe = find()?;
    let out = Command::new(exe).arg("version").output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().next().map(str::to_owned)
}

pub struct Rclone {
    child: Child,
    base: String,
    auth: String,
    agent: ureq::Agent,
}

impl Rclone {
    pub fn start() -> Result<Self, String> {
        let exe = find().ok_or("rclone not found. Install it from rclone.org or set RCLONE_PATH.")?;
        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map_err(|e| e.to_string())?
            .port();
        let user = "pfc";
        let pass = random_token();
        let child = Command::new(exe)
            .args(["rcd", "--rc-addr", &format!("127.0.0.1:{port}")])
            .args(["--rc-user", user, "--rc-pass", &pass])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("could not start rclone: {e}"))?;
        let mut me = Self {
            child,
            base: format!("http://127.0.0.1:{port}"),
            auth: format!("Basic {}", base64(format!("{user}:{pass}").as_bytes())),
            agent: ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(2)).build(),
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if me.call("rc/noop", json!({})).is_ok() {
                return Ok(me);
            }
            if Instant::now() > deadline {
                let _ = me.child.kill();
                return Err("rclone did not start in time".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn call(&self, method: &str, body: Value) -> Result<Value, String> {
        let resp = self
            .agent
            .post(&format!("{}/{method}", self.base))
            .set("Authorization", &self.auth)
            .send_json(body);
        match resp {
            Ok(r) => r.into_json().map_err(|e| e.to_string()),
            Err(ureq::Error::Status(_, r)) => {
                let v: Value = r.into_json().unwrap_or_default();
                Err(v["error"].as_str().unwrap_or("rclone error").to_owned())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn file_op(&self, method: &str, src: &Path, dst: &Path) -> Result<(), String> {
        let split = |p: &Path| -> Result<(String, String), String> {
            let dir = p.parent().ok_or("path has no parent")?;
            let name = p.file_name().ok_or("path has no file name")?;
            Ok((dir.to_string_lossy().into_owned(), name.to_string_lossy().into_owned()))
        };
        let (src_fs, src_remote) = split(src)?;
        let (dst_fs, dst_remote) = split(dst)?;
        self.call(
            method,
            json!({ "srcFs": src_fs, "srcRemote": src_remote, "dstFs": dst_fs, "dstRemote": dst_remote }),
        )
        .map(|_| ())
    }
}

impl Transfer for Rclone {
    fn copy(&self, src: &Path, dst: &Path) -> Result<(), String> {
        self.file_op("operations/copyfile", src, dst)
    }
    fn move_file(&self, src: &Path, dst: &Path) -> Result<(), String> {
        self.file_op("operations/movefile", src, dst)
    }
}

impl Drop for Rclone {
    fn drop(&mut self) {
        let _ = self.call("core/quit", json!({}));
        std::thread::sleep(Duration::from_millis(200));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn random_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    (0..2)
        .map(|_| {
            let mut h = RandomState::new().build_hasher();
            // RandomState is seeded from OS randomness; mix in the clock too.
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
            h.write_u128(now.unwrap_or_default().as_nanos());
            format!("{:016x}", h.finish())
        })
        .collect()
}

fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
    }
}
