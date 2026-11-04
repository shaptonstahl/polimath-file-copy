import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";

type FileMeta = { size: number; mtime_ms: number; cloud_only: boolean };
type Action = { kind: "create" } | { kind: "overwrite" } | { kind: "skip"; reason: string };
type PlanItem = { source: string; dest: string; rel: string; src: FileMeta; dst: FileMeta | null; action: Action };
type Plan = {
  items: PlanItem[];
  errors: string[];
  lookup: { dirs_listed: number; files_statted: number; dirs_missing: number };
};
type Progress = { index: number; done: number; total: number; source: string; error: string | null };
type Summary = { ok: number; failed: number; cancelled: number };

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

let sources: string[] = [];
let target: string | null = null;
let plan: Plan | null = null;

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

function invalidate() {
  plan = null;
  renderSources();
  targetBox.textContent = target ?? "";
  targetBox.title = target ?? "";
  runBtn.disabled = true;
  table.hidden = true;
  summaryEl.textContent = "";
  errorsEl.replaceChildren();
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
    if (zone === "target" && p.paths.length) setTarget(p.paths[0]);
    else if (p.paths.length) addSources(p.paths);
  }
});

// ---------- options ----------

function options() {
  return {
    rule: $<HTMLSelectElement>("rule").value,
    tolerance_ms: Math.max(0, Number($<HTMLInputElement>("tolerance").value) || 0) * 1000,
    skip_cloud_only: $<HTMLInputElement>("skip-cloud").checked,
  };
}
for (const id of ["rule", "tolerance", "skip-cloud"]) $(id).addEventListener("change", invalidate);

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

previewBtn.onclick = async () => {
  if (!sources.length || !target) {
    summaryEl.textContent = "Add at least one source and choose a target folder.";
    return;
  }
  previewBtn.disabled = true;
  summaryEl.textContent = "Reading metadata…";
  try {
    plan = await invoke<Plan>("plan", { req: { sources, target, options: options() } });
  } catch (e) {
    summaryEl.textContent = String(e);
    previewBtn.disabled = false;
    return;
  }
  previewBtn.disabled = false;

  const n = { create: 0, overwrite: 0, skip: 0 };
  let bytes = 0;
  let cloud = 0;
  for (const it of plan.items) {
    n[it.action.kind]++;
    if (it.action.kind !== "skip") bytes += it.src.size;
    if (it.src.cloud_only) cloud++;
  }
  const l = plan.lookup;
  summaryEl.textContent =
    `${n.create} new, ${n.overwrite} to overwrite, ${n.skip} skipped — ${fmtSize(bytes)} to transfer.` +
    (cloud ? ` ${cloud} source files are cloud-only.` : "") +
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
        cell(fmtSize(it.src.size), "num"),
        cell(fmtTime(it.src.mtime_ms)),
        cell(it.dst ? fmtSize(it.dst.size) : "", "num"),
        cell(it.dst ? fmtTime(it.dst.mtime_ms) : ""),
        cell(it.action.kind === "skip" ? it.action.reason : ""),
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
  runBtn.disabled = n.create + n.overwrite === 0;
};

// ---------- run ----------

let failures: string[] = [];

runBtn.onclick = async () => {
  if (!plan) return;
  const jobs = plan.items
    .filter((it) => it.action.kind !== "skip")
    .map((it) => ({ source: it.source, dest: it.dest }));
  failures = [];
  showErrors([]);
  progressEl.max = jobs.length;
  progressEl.value = 0;
  progressEl.hidden = false;
  setRunning(true);
  try {
    await invoke("execute", {
      req: {
        jobs,
        operation: $<HTMLSelectElement>("operation").value,
        engine: $<HTMLSelectElement>("engine").value,
        parallel: Number($<HTMLInputElement>("parallel").value) || 4,
      },
    });
  } catch (e) {
    summaryEl.textContent = String(e);
    setRunning(false);
  }
};

cancelBtn.onclick = () => invoke("cancel");

function setRunning(on: boolean) {
  previewBtn.disabled = on;
  runBtn.disabled = true;
  cancelBtn.hidden = !on;
}

listen<Progress>("transfer-progress", ({ payload: p }) => {
  progressEl.value = p.done;
  summaryEl.textContent = `${p.done} / ${p.total}: ${p.source}`;
  if (p.error) {
    failures.push(`${p.source}: ${p.error}`);
    showErrors(failures);
  }
});

listen<{ Ok: Summary } | { Err: string }>("transfer-done", ({ payload }) => {
  setRunning(false);
  progressEl.hidden = true;
  plan = null;
  if ("Err" in payload) summaryEl.textContent = payload.Err;
  else {
    const s = payload.Ok;
    summaryEl.textContent = `Done: ${s.ok} succeeded, ${s.failed} failed, ${s.cancelled} cancelled. Preview again to see the new state.`;
  }
});

invalidate();
