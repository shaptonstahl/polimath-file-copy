import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";

type FileMeta = { size: number; mtime_ms: number; cloud_only: boolean };
type Action = { kind: "create" } | { kind: "overwrite" } | { kind: "skip"; reason: string };
type PlanItem = {
  source: string;
  dest: string;
  rel: string;
  src: FileMeta;
  dst: FileMeta | null;
  action: Action;
  rename_only: boolean;
  icloud_stub: boolean;
};
type Level = "info" | "warn" | "danger";
type Warning = { level: Level; code: string; message: string };
type Environment = {
  os: string;
  version: string;
  major: number;
  icloud_stubs: boolean;
  cloud_detection: boolean;
  notes: Warning[];
};
type Plan = {
  id: number;
  log_path: string | null;
  log_error: string | null;
  items: PlanItem[];
  errors: string[];
  warnings: Warning[];
  lookup: { dirs_listed: number; files_statted: number; dirs_missing: number };
};
type Progress = { index: number; done: number; total: number; source: string; error: string | null };
type Status = "preview" | "success" | "partial" | "failed" | "cancelled";
type Outcome = {
  status: Status;
  title: string;
  message: string;
  planned: number;
  done: number;
  failed: number;
  not_attempted: number;
  skipped: number;
  log_path: string | null;
  log_error: string | null;
};
type Settings = { log_enabled: boolean; log_dir: string | null };
type SettingsView = { settings: Settings; default_log_dir: string; effective_log_dir: string };

const MAX_ROWS = 2000;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const sourceList = $<HTMLUListElement>("source-list");
const targetBox = $<HTMLDivElement>("target-box");
const runBtn = $<HTMLButtonElement>("run");
const previewBtn = $<HTMLButtonElement>("preview");
const cancelBtn = $<HTMLButtonElement>("cancel");
const progressEl = $<HTMLProgressElement>("progress");
const summaryEl = $<HTMLDivElement>("summary");
const errorsEl = $<HTMLUListElement>("errors");
const table = $<HTMLTableElement>("plan-table");
const ackBox = $<HTMLInputElement>("ack");
const logBox = $<HTMLInputElement>("log");

let sources: string[] = [];
let target: string | null = null;
let plan: Plan | null = null;
let pathWarnings: Warning[] = [];
let running = false;

// ---------- guardrails ----------

const ICON: Record<Level, string> = { info: "ℹ️", warn: "⚠️", danger: "⛔" };
const ORDER: Record<Level, number> = { danger: 0, warn: 1, info: 2 };

function warningItems(list: Warning[]) {
  return list.map((w) => {
    const li = document.createElement("li");
    li.className = w.level;
    const tag = document.createElement("span");
    tag.className = "tag";
    tag.textContent = ICON[w.level];
    li.append(tag, w.message);
    return li;
  });
}

/** Path warnings (before Preview) plus plan warnings (before Run). */
function allWarnings(): Warning[] {
  const seen = new Set<string>();
  const out: Warning[] = [];
  for (const w of [...pathWarnings, ...(plan?.warnings ?? [])]) {
    // The same library can be reported from both the paths and the plan.
    const key = w.code === "library_source" ? w.code : w.code + w.message;
    if (!seen.has(key)) {
      seen.add(key);
      out.push(w);
    }
  }
  return out.sort((a, b) => ORDER[a.level] - ORDER[b.level]);
}

const hasDanger = () => allWarnings().some((w) => w.level === "danger");
const runnableCount = () => (plan ? plan.items.filter((it) => it.action.kind !== "skip").length : 0);

function updateRunEnabled() {
  runBtn.disabled = running || runnableCount() === 0 || (hasDanger() && !ackBox.checked);
}
ackBox.addEventListener("change", updateRunEnabled);

function renderWarnings() {
  const list = allWarnings();
  $("warnings").replaceChildren(...warningItems(list));
  $("warnings-wrap").hidden = list.length === 0;
  const danger = list.some((w) => w.level === "danger");
  $("ack-wrap").hidden = !danger || !plan;
  if (!danger) ackBox.checked = false;
  updateRunEnabled();
}

let checkSeq = 0;
async function refreshPathWarnings() {
  const seq = ++checkSeq;
  const w = await invoke<Warning[]>("check", {
    req: {
      sources,
      target,
      operation: $<HTMLSelectElement>("operation").value,
      engine: $<HTMLSelectElement>("engine").value,
    },
  }).catch(() => [] as Warning[]);
  if (seq !== checkSeq) return; // a newer check superseded this one
  pathWarnings = w;
  renderWarnings();
}

invoke<Environment>("environment").then((env) => {
  const name: Record<string, string> = { windows: "Windows", macos: "macOS", linux: "Linux" };
  $("env-line").textContent =
    `${name[env.os] ?? env.os} ${env.version} · cloud-only detection ${env.cloud_detection ? "on" : "not available"}` +
    (env.icloud_stubs ? " · older iCloud Drive format (.icloud stand-ins)" : "");
  const notes = $("env-notes");
  notes.replaceChildren(...warningItems(env.notes));
  notes.hidden = env.notes.length === 0;
});

// ---------- settings ----------

let settings: Settings = { log_enabled: false, log_dir: null };

function showSettings(v: SettingsView) {
  settings = v.settings;
  logBox.checked = v.settings.log_enabled;
  $<HTMLInputElement>("log-dir").value = v.effective_log_dir;
  $("log-dir-default").textContent = v.settings.log_dir
    ? `Default: ${v.default_log_dir}`
    : "This is the default location for your user account.";
  $("reset-log-dir").hidden = !v.settings.log_dir;
}

async function saveSettings(next: Settings) {
  $("settings-error").textContent = "";
  try {
    showSettings(await invoke<SettingsView>("set_settings", { settings: next }));
  } catch (e) {
    $("settings-error").textContent = String(e);
    logBox.checked = settings.log_enabled;
  }
}

invoke<SettingsView>("get_settings").then(showSettings);
logBox.addEventListener("change", () => saveSettings({ ...settings, log_enabled: logBox.checked }));
$("open-settings").onclick = () => $<HTMLDialogElement>("settings-dialog").showModal();
$("choose-log-dir").onclick = async () => {
  const r = await open({ multiple: false, directory: true, title: "Choose log folder" });
  if (typeof r === "string") saveSettings({ ...settings, log_dir: r });
};
$("reset-log-dir").onclick = () => saveSettings({ ...settings, log_dir: null });
$("open-log-dir").onclick = () =>
  invoke("open_log_folder").catch((e) => ($("settings-error").textContent = String(e)));

// ---------- end-of-job popup ----------

let shownLog: string | null = null;

function showOutcome(o: Outcome) {
  const dlg = $<HTMLDialogElement>("outcome-dialog");
  dlg.dataset.status = o.status;
  const icon: Record<Status, string> = { preview: "🔍", success: "✅", partial: "⚠️", failed: "⛔", cancelled: "⏹" };
  $("outcome-title").textContent = `${icon[o.status]} ${o.title}`;
  $("outcome-message").textContent = o.message;

  const verb = $<HTMLSelectElement>("operation").value === "move" ? "moved" : "copied";
  const lines =
    o.status === "preview"
      ? [`${o.planned} to transfer`, `${o.skipped} to skip`]
      : [`${o.done} of ${o.planned} ${verb}`, `${o.failed} failed`, `${o.not_attempted} not attempted`, `${o.skipped} skipped as planned`];
  $("outcome-counts").replaceChildren(
    ...lines.map((t) => {
      const li = document.createElement("li");
      li.textContent = t;
      return li;
    }),
  );

  shownLog = o.log_path;
  $("outcome-log").hidden = !o.log_path;
  $("outcome-log-link").textContent = o.log_path ?? "";
  $("outcome-log-link").title = "Open this log";
  $("outcome-log-error").textContent = o.log_error ? `Log problem: ${o.log_error}` : "";
  dlg.showModal();
}

const openShownLog = (e?: Event) => {
  e?.preventDefault();
  if (shownLog) invoke("open_log", { path: shownLog }).catch((err) => ($("outcome-log-error").textContent = String(err)));
};
$("outcome-log-link").onclick = openShownLog;
$("outcome-open-log").onclick = () => openShownLog();
$("outcome-reveal-log").onclick = () => {
  if (shownLog) invoke("reveal_log", { path: shownLog }).catch((err) => ($("outcome-log-error").textContent = String(err)));
};

// ---------- source & target lists ----------

function addSources(paths: string[]) {
  for (const p of paths) if (!sources.includes(p)) sources.push(p);
  invalidate();
}

function renderSources() {
  sourceList.replaceChildren(
    ...sources.map((p, i) => {
      const li = document.createElement("li");
      const span = document.createElement("span");
      span.textContent = p;
      span.title = p;
      const rm = document.createElement("button");
      rm.className = "ghost icon";
      rm.textContent = "×";
      rm.title = "Remove";
      rm.onclick = () => {
        sources.splice(i, 1);
        invalidate();
      };
      li.append(span, rm);
      return li;
    }),
  );
  $("source-count").textContent = sources.length ? `${sources.length}` : "";
}

function setTarget(p: string | null) {
  target = p;
  invalidate();
}

/** Any input changed: drop the plan and re-check the paths. */
function invalidate() {
  plan = null;
  ackBox.checked = false;
  renderSources();
  targetBox.textContent = target ?? "";
  targetBox.title = target ?? "";
  table.hidden = true;
  summaryEl.textContent = "";
  errorsEl.replaceChildren();
  renderWarnings();
  refreshPathWarnings();
}

$("add-files").onclick = async () => {
  const r = await open({ multiple: true, directory: false, title: "Add source files" });
  if (r) addSources(Array.isArray(r) ? r : [r]);
};
$("add-folders").onclick = async () => {
  const r = await open({ multiple: true, directory: true, title: "Add source folders" });
  if (r) addSources(Array.isArray(r) ? r : [r]);
};
$("clear-sources").onclick = () => {
  sources = [];
  invalidate();
};
$("choose-target").onclick = async () => {
  const r = await open({ multiple: false, directory: true, title: "Choose target folder" });
  if (typeof r === "string") setTarget(r);
};

// ---------- drag and drop (native paths come from Tauri, not the DOM) ----------

function zoneAt(x: number, y: number): "source" | "target" | null {
  const hit = (el: HTMLElement) => {
    const r = el.getBoundingClientRect();
    return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  };
  if (hit($("target-panel"))) return "target";
  if (hit($("source-panel"))) return "source";
  return null;
}

function highlight(zone: "source" | "target" | null) {
  $("source-panel").classList.toggle("over", zone === "source");
  $("target-panel").classList.toggle("over", zone === "target");
}

getCurrentWebview().onDragDropEvent((e) => {
  const p = e.payload;
  if (p.type === "leave") return highlight(null);
  const scale = window.devicePixelRatio || 1;
  const zone = zoneAt(p.position.x / scale, p.position.y / scale);
  if (p.type === "over" || p.type === "enter") return highlight(zone);
  if (p.type === "drop") {
    highlight(null);
    if (running) return;
    if (zone === "target" && p.paths.length) setTarget(p.paths[0]);
    else if (p.paths.length) addSources(p.paths);
  }
});

// ---------- options ----------

function options() {
  return {
    operation: $<HTMLSelectElement>("operation").value,
    rule: $<HTMLSelectElement>("rule").value,
    tolerance_ms: Math.max(0, Number($<HTMLInputElement>("tolerance").value) || 0) * 1000,
    skip_cloud_only: $<HTMLInputElement>("skip-cloud").checked,
  };
}
for (const id of ["operation", "engine", "rule", "tolerance", "skip-cloud"]) {
  $(id).addEventListener("change", invalidate);
}

invoke<string | null>("rclone_version").then((v) => {
  if (!v) return;
  const opt = $<HTMLSelectElement>("engine").querySelector<HTMLOptionElement>('option[value="rclone"]')!;
  opt.disabled = false;
  opt.textContent = v;
});

// ---------- preview ----------

const fmtSize = (n: number) => {
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) (n /= 1024), i++;
  return `${i ? n.toFixed(1) : n} ${u[i]}`;
};
const fmtTime = (ms: number) => new Date(ms).toLocaleString();

function cell(text: string, cls?: string) {
  const td = document.createElement("td");
  td.textContent = text;
  if (cls) td.className = cls;
  return td;
}

function showErrors(errs: string[]) {
  errorsEl.replaceChildren(
    ...errs.map((e) => {
      const li = document.createElement("li");
      li.textContent = e;
      return li;
    }),
  );
}

function note(it: PlanItem) {
  if (it.action.kind === "skip") return it.action.reason;
  if (it.rename_only) return "moved by renaming within the cloud folder (no download)";
  if (it.src.cloud_only) return "cloud-only: will be downloaded";
  return "";
}

previewBtn.onclick = async () => {
  if (!sources.length || !target) {
    summaryEl.textContent = "Add at least one source and choose a target folder.";
    return;
  }
  previewBtn.disabled = true;
  summaryEl.textContent = "Reading metadata…";
  try {
    plan = await invoke<Plan>("plan", { req: { sources, target, options: options(), log: logBox.checked } });
  } catch (e) {
    summaryEl.textContent = String(e);
    return;
  } finally {
    previewBtn.disabled = false;
  }

  const n = { create: 0, overwrite: 0, skip: 0 };
  let bytes = 0;
  for (const it of plan.items) {
    n[it.action.kind]++;
    if (it.action.kind !== "skip" && !it.rename_only) bytes += it.src.size;
  }
  const l = plan.lookup;
  summaryEl.textContent =
    `${n.create} new, ${n.overwrite} to overwrite, ${n.skip} skipped — ${fmtSize(bytes)} to transfer.` +
    ` Target check: ${l.dirs_listed} folders listed, ${l.files_statted} files looked up, ${l.dirs_missing} folders new.`;
  showErrors(plan.errors);

  const tbody = table.tBodies[0];
  tbody.replaceChildren(
    ...plan.items.slice(0, MAX_ROWS).map((it) => {
      const tr = document.createElement("tr");
      tr.className = it.action.kind;
      tr.append(
        cell(it.action.kind, "badge"),
        cell(it.rel + (it.src.cloud_only ? " ☁" : ""), "path"),
        cell(it.icloud_stub ? "?" : fmtSize(it.src.size), "num"),
        cell(fmtTime(it.src.mtime_ms)),
        cell(it.dst ? fmtSize(it.dst.size) : "", "num"),
        cell(it.dst ? fmtTime(it.dst.mtime_ms) : ""),
        cell(note(it)),
      );
      return tr;
    }),
  );
  if (plan.items.length > MAX_ROWS) {
    const tr = document.createElement("tr");
    const td = cell(`…and ${plan.items.length - MAX_ROWS} more`);
    td.colSpan = 7;
    tr.append(td);
    tbody.append(tr);
  }
  table.hidden = plan.items.length === 0;
  renderWarnings();

  // Logging was requested: confirm the preview log (or why it failed).
  if (plan.log_path || plan.log_error) {
    const verb = options().operation === "move" ? "move" : "copy";
    const go = n.create + n.overwrite;
    showOutcome({
      status: "preview",
      title: "Preview complete",
      message: `PREVIEW ONLY - no action was taken. ${go} file${go === 1 ? "" : "s"} would ${verb}; ${n.skip} would be skipped.`,
      planned: go,
      done: 0,
      failed: 0,
      not_attempted: 0,
      skipped: n.skip,
      log_path: plan.log_path,
      log_error: plan.log_error,
    });
  }
};

// ---------- run ----------

let failures: string[] = [];

function setRunning(on: boolean) {
  running = on;
  previewBtn.disabled = on;
  cancelBtn.hidden = !on;
  updateRunEnabled();
}

runBtn.onclick = async () => {
  if (!plan || runBtn.disabled) return;
  const total = plan.items.filter((it) => it.action.kind !== "skip").length;
  failures = [];
  showErrors([]);
  progressEl.max = total;
  progressEl.value = 0;
  progressEl.hidden = false;
  setRunning(true);
  try {
    await invoke("execute", {
      req: {
        plan_id: plan.id,
        engine: $<HTMLSelectElement>("engine").value,
        parallel: Number($<HTMLInputElement>("parallel").value) || 4,
        log: logBox.checked,
      },
    });
  } catch (e) {
    summaryEl.textContent = String(e);
    progressEl.hidden = true;
    setRunning(false);
  }
};

cancelBtn.onclick = () => invoke("cancel");

listen<Progress>("transfer-progress", ({ payload: p }) => {
  progressEl.value = p.done;
  summaryEl.textContent = `${p.done} / ${p.total}: ${p.source}`;
  if (p.error) {
    failures.push(`${p.source}: ${p.error}`);
    showErrors(failures);
  }
});

listen<Outcome>("job-finished", ({ payload: o }) => {
  progressEl.hidden = true;
  plan = null; // the file system changed; force a fresh preview
  setRunning(false);
  renderWarnings();
  summaryEl.textContent = `${o.title}: ${o.message} Preview again to see the new state.`;
  showOutcome(o);
});

invalidate();
