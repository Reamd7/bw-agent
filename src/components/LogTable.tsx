import { For, Show, createSignal, onMount, onCleanup } from "solid-js";
import type { AccessLogEntry, ProcessInfo } from "../lib/tauri";

interface LogTableProps {
  logs: AccessLogEntry[];
}

// --- Utility functions ---

const extractExeName = (fullPath: string) => {
  const parts = fullPath.split(/[/\\]/);
  return parts[parts.length - 1] || fullPath;
};

const formatTime = (timestamp: string) => {
  try {
    return new Date(timestamp).toLocaleString();
  } catch {
    return timestamp;
  }
};

/** Get meaningful initiator: skip "pid:XXXX" entries (process already exited). */
const getInitiator = (processChain: ProcessInfo[], fallbackExe: string): string => {
  if (!processChain || processChain.length === 0) {
    return extractExeName(fallbackExe);
  }
  for (const proc of processChain) {
    const name = extractExeName(proc.exe);
    if (!name.startsWith("pid:")) {
      return name;
    }
  }
  // All entries are pid:XXX, use first one
  return extractExeName(processChain[0].exe);
};

/** Shorten fingerprint for display: "SHA256:vAbpjk...yuU" */
const shortenFingerprint = (fp: string): string => {
  if (!fp) return "";
  // "SHA256:abcdef..." → keep prefix + first 8 + last 4
  const colonIdx = fp.indexOf(":");
  if (colonIdx >= 0) {
    const prefix = fp.slice(0, colonIdx + 1);
    const hash = fp.slice(colonIdx + 1);
    if (hash.length > 12) {
      return `${prefix}${hash.slice(0, 8)}...${hash.slice(-4)}`;
    }
  }
  if (fp.length > 20) {
    return `${fp.slice(0, 16)}...${fp.slice(-4)}`;
  }
  return fp;
};

// Parse ssh cmdline to extract operation type and target.
interface SshInfo {
  operation: string;
  target: string;
}

const SSH_OP_MAP: Record<string, string> = {
  "git-upload-pack": "fetch",
  "git-receive-pack": "push",
  "git-upload-archive": "archive",
};

const parseSshCmdline = (processChain: ProcessInfo[]): SshInfo => {
  if (!processChain || processChain.length === 0) {
    return { operation: "unknown", target: "unknown" };
  }

  // Detect git-sign: bw-agent-git-sign in the process chain indicates a git
  // commit signing operation, not an SSH connection.
  for (const proc of processChain) {
    const exeName = extractExeName(proc.exe).toLowerCase();
    if (exeName.includes("git-sign")) {
      // Extract -Y action (sign, verify, find-principals, etc.)
      const actionMatch = proc.cmdline.match(/-Y\s+(\S+)/);
      const action = actionMatch ? actionMatch[1] : "sign";

      // Look for the parent git process to determine what triggered signing
      // (commit, tag, merge, etc.) and the working directory.
      let gitCmd = "";
      let cwd = "";
      for (const parent of processChain) {
        const parentExe = extractExeName(parent.exe).toLowerCase();
        if (parentExe === "git.exe" || parentExe === "git") {
          const cmdMatch = parent.cmdline.match(/^git\s+(\S+)/);
          if (cmdMatch) gitCmd = cmdMatch[1];
          cwd = parent.cwd || "";
          break;
        }
      }

      const operation = gitCmd ? `git ${gitCmd} (${action})` : `git ${action}`;
      const target = cwd || "unknown";
      return { operation, target };
    }
  }

  const sshNode = processChain[processChain.length - 1];
  const cmdline = sshNode.cmdline;

  // Look for quoted command: "git-upload-pack 'owner/repo.git'"
  const quotedMatch = cmdline.match(/"(\S+)\s+'([^']+)'/);
  if (quotedMatch) {
    const op = quotedMatch[1];
    const repo = quotedMatch[2];
    return {
      operation: SSH_OP_MAP[op] || op,
      target: repo,
    };
  }

  // Also try: git-upload-pack 'repo.git' (without outer quotes)
  const unquotedMatch = cmdline.match(/(git-\S+)\s+'([^']+)'/);
  if (unquotedMatch) {
    const op = unquotedMatch[1];
    const repo = unquotedMatch[2];
    return {
      operation: SSH_OP_MAP[op] || op,
      target: repo,
    };
  }

  // ls-remote pattern from git cmdline (not ssh cmdline)
  // Check parent processes for ls-remote
  for (const proc of processChain) {
    if (proc.cmdline.includes("ls-remote")) {
      // Still parse target from ssh node
      const hostMatch = cmdline.match(/(\S+@\S+)/);
      return {
        operation: "ls-remote",
        target: hostMatch ? hostMatch[1] : cmdline,
      };
    }
  }

  // Plain SSH: user@host
  const hostMatch = cmdline.match(/(\S+@\S+)/);
  if (hostMatch) {
    return { operation: "ssh", target: hostMatch[1] };
  }

  return { operation: "ssh", target: cmdline };
};

// --- Collapsible cmdline component ---

function CmdlineText(props: { text: string }) {
  const [expanded, setExpanded] = createSignal(false);
  const isLong = () => props.text.length > 120;

  return (
    <Show
      when={isLong()}
      fallback={<div class="mt-0.5 font-mono text-xs break-all" style={`color: var(--text-secondary)`}>{props.text}</div>}
    >
      <div class="mt-0.5 font-mono text-xs" style={`color: var(--text-secondary)`}>
        <Show
          when={expanded()}
          fallback={
            <div class="break-all">
              {props.text.slice(0, 120)}...
              <button
                class="ml-1 font-sans text-xs"
                style={`color: var(--brand-500)`}
                onClick={(e) => {
                  e.stopPropagation();
                  setExpanded(true);
                }}
              >
                Show more
              </button>
            </div>
          }
        >
          <div class="break-all">
            {props.text}
            <button
              class="ml-1 font-sans text-xs"
              style={`color: var(--brand-500)`}
              onClick={(e) => {
                e.stopPropagation();
                setExpanded(false);
              }}
            >
              Show less
            </button>
          </div>
        </Show>
      </div>
    </Show>
  );
}

// --- Detail Modal ---

function LogDetailModal(props: { log: AccessLogEntry; onClose: () => void }) {
  const sshInfo = () => parseSshCmdline(props.log.process_chain);

  // Lock body scroll while modal is open
  onMount(() => {
    const original = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    onCleanup(() => {
      document.body.style.overflow = original;
    });
  });

  // Close on Escape
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") props.onClose();
  };

  return (
    <div
      class="overlay"
      onClick={props.onClose}
      onKeyDown={handleKeyDown}
      tabIndex={-1}
      ref={(el) => el?.focus()}
    >
      <div class="modal" style={{ "max-width": "520px" }} onClick={(e) => e.stopPropagation()}>
        {/* Header */}
        <div class="flex items-center justify-between px-6 pt-6 pb-0">
          <div class="flex items-center gap-3">
            <h3 class="text-base font-semibold" style={`color: var(--text-primary)`}>Log Detail</h3>
            <Show when={props.log.approved && props.log.auto_approved} fallback={
              <span class={`badge ${props.log.approved ? "badge-success" : "badge-danger"}`}>
                {props.log.approved ? "Approved" : "Denied"}
              </span>
            }>
              <span class="badge" style={{ background: "var(--brand-50)", color: "var(--brand-700)" }}>
                Auto-Approved
              </span>
            </Show>
          </div>
          <button
            class="btn-ghost"
            style={{ "border-radius": "var(--radius-md)", padding: "6px" }}
            onClick={props.onClose}
          >
            <svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        {/* Scrollable content */}
        <div class="overflow-y-auto px-6 py-4" style={{ "max-height": "60vh" }}>
          <div class="space-y-3">
            <div class="flex justify-between text-sm">
              <span style={`color: var(--text-tertiary)`}>Time</span>
              <span style={`color: var(--text-primary)`}>{formatTime(props.log.timestamp)}</span>
            </div>
            <div class="flex justify-between text-sm">
              <span style={`color: var(--text-tertiary)`}>Operation</span>
              <span class="font-medium" style={`color: var(--text-primary)`}>{sshInfo().operation}</span>
            </div>
            <div class="flex justify-between text-sm">
              <span class="shrink-0" style={`color: var(--text-tertiary)`}>Target</span>
              <span class="font-mono text-xs text-right max-w-[300px] break-all" style={`color: var(--text-secondary)`}>
                {sshInfo().target}
              </span>
            </div>
            <div class="flex justify-between text-sm">
              <span style={`color: var(--text-tertiary)`}>Key</span>
              <span style={`color: var(--text-primary)`}>{props.log.key_name}</span>
            </div>
            <div>
              <span class="text-sm" style={`color: var(--text-tertiary)`}>Fingerprint</span>
              <div class="mt-1 font-mono text-xs rounded-lg p-2.5 break-all" style={`background: var(--bg-secondary); color: var(--text-secondary)`}>
                {props.log.key_fingerprint}
              </div>
            </div>
            <Show when={props.log.auto_approved}>
              <div class="flex justify-between text-sm">
                <span style={`color: var(--text-tertiary)`}>Authorization</span>
                <span class="badge text-xs" style={{ background: "var(--brand-50)", color: "var(--brand-700)" }}>
                  Session auto-approval
                </span>
              </div>
            </Show>

            {/* Process Chain */}
            <Show when={props.log.process_chain && props.log.process_chain.length > 0}>
              <div>
                <span class="text-sm" style={`color: var(--text-tertiary)`}>Process Chain</span>
                <div class="mt-2 space-y-1">
                  <For each={props.log.process_chain}>
                    {(proc, index) => (
                      <div class="flex items-start gap-2 rounded-lg p-2.5 text-xs" style={`background: var(--bg-secondary)`}>
                        <span class="shrink-0 font-mono" style={`color: var(--text-tertiary)`}>{index() + 1}.</span>
                        <div class="min-w-0 flex-1">
                          <div class="flex items-center gap-2">
                            <span class="font-semibold" style={`color: var(--text-primary)`}>
                              {extractExeName(proc.exe)}
                            </span>
                            <span style={`color: var(--text-tertiary)`}>PID: {proc.pid}</span>
                          </div>
                          <Show when={proc.cmdline && proc.cmdline !== "unknown"}>
                            <CmdlineText text={proc.cmdline} />
                          </Show>
                          <Show
                            when={
                              !proc.exe.startsWith("pid:") &&
                              extractExeName(proc.exe) !== proc.exe
                            }
                          >
                            <div class="mt-0.5 break-all" style={`color: var(--text-tertiary)`}>{proc.exe}</div>
                          </Show>
                        </div>
                      </div>
                    )}
                  </For>
                </div>
              </div>
            </Show>

            {/* Fallback for old logs without process chain */}
            <Show when={!props.log.process_chain || props.log.process_chain.length === 0}>
              <div class="flex justify-between text-sm">
                <span style={`color: var(--text-tertiary)`}>Client</span>
                <span style={`color: var(--text-primary)`} title={props.log.client_exe}>
                  {extractExeName(props.log.client_exe)} (PID: {props.log.client_pid})
                </span>
              </div>
            </Show>
          </div>
        </div>

        {/* Footer */}
        <div class="px-6 pb-6">
          <button
            class="btn btn-secondary w-full"
            onClick={props.onClose}
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

// --- Log Table (Card List) ---

export function LogTable(props: LogTableProps) {
  const [selectedLog, setSelectedLog] = createSignal<AccessLogEntry | null>(null);

  return (
    <>
      <Show
        when={props.logs.length > 0}
        fallback={
          <div class="card empty-state">
            <svg fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <h3>No access logs</h3>
            <p>Logs will appear when keys are accessed.</p>
          </div>
        }
      >
        <div class="space-y-2">
          <For each={props.logs}>
            {(log) => {
              const sshInfo = parseSshCmdline(log.process_chain);
              const initiator = getInitiator(log.process_chain, log.client_exe);

              return (
                <div
                  class="card cursor-pointer"
                  style={{ padding: "14px 20px" }}
                  onClick={() => setSelectedLog(log)}
                >
                  <div class="flex items-center gap-4">
                    {/* Status indicator */}
                    <div
                      class="flex h-8 w-8 items-center justify-center rounded-lg shrink-0"
                      style={`background: ${log.approved ? "var(--success-bg)" : "var(--danger-bg)"}`}
                    >
                      <svg class="h-4 w-4" style={`color: ${log.approved ? "var(--success)" : "var(--danger)"}`} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                        {log.approved
                          ? <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                          : <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                        }
                      </svg>
                    </div>

                    <div class="min-w-0 flex-1">
                      <div class="flex items-center gap-2">
                        <span class="text-sm font-medium" style={`color: var(--text-primary)`}>{initiator}</span>
                        <span class="text-xs" style={`color: var(--text-tertiary)`}>&middot;</span>
                        <span class="text-sm font-medium" style={`color: var(--text-secondary)`}>{sshInfo.operation}</span>
                      </div>
                      <div class="flex items-center gap-2 mt-0.5">
                        <Show when={sshInfo.target !== "unknown"}>
                          <span class="text-xs font-mono truncate" style={`color: var(--text-tertiary)`}>{sshInfo.target}</span>
                        </Show>
                      </div>
                    </div>

                    <span class="text-xs shrink-0" style={`color: var(--text-tertiary)`}>
                      {formatTime(log.timestamp)}
                    </span>

                    <Show when={log.approved && log.auto_approved} fallback={
                      <span class={`badge ${log.approved ? "badge-success" : "badge-danger"} shrink-0`}>
                        {log.approved ? "Approved" : "Denied"}
                      </span>
                    }>
                      <span class="badge shrink-0" style={{ background: "var(--brand-50)", color: "var(--brand-700)" }}>
                        Auto-Approved
                      </span>
                    </Show>
                  </div>
                </div>
              );
            }}
          </For>
        </div>
      </Show>

      {/* Detail Modal */}
      <Show when={selectedLog()}>
        {(log) => <LogDetailModal log={log()} onClose={() => setSelectedLog(null)} />}
      </Show>
    </>
  );
}
