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
      return { operation: `git ${action}`, target: "commit" };
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
      fallback={<div class="mt-0.5 font-mono text-gray-600 break-all">{props.text}</div>}
    >
      <div class="mt-0.5 font-mono text-gray-600">
        <Show
          when={expanded()}
          fallback={
            <div class="break-all">
              {props.text.slice(0, 120)}...
              <button
                class="ml-1 text-blue-500 hover:text-blue-700 font-sans text-xs"
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
              class="ml-1 text-blue-500 hover:text-blue-700 font-sans text-xs"
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
      class="fixed inset-0 z-50 flex items-center justify-center overflow-hidden bg-gray-900/50 p-4"
      onClick={props.onClose}
      onKeyDown={handleKeyDown}
      tabIndex={-1}
      ref={(el) => el?.focus()}
    >
      <div
        class="relative w-full max-w-lg max-h-[85vh] flex flex-col rounded-xl bg-white shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header - fixed */}
        <div class="flex items-center justify-between p-6 pb-0">
          <div class="flex items-center gap-3">
            <h3 class="text-lg font-bold text-gray-900">Log Detail</h3>
            <span
              class={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium ${
                props.log.approved
                  ? "bg-green-100 text-green-800"
                  : "bg-red-100 text-red-800"
              }`}
            >
              {props.log.approved ? "Approved" : "Denied"}
            </span>
          </div>
          <button
            class="rounded-lg p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600"
            onClick={props.onClose}
          >
            <svg class="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M6 18L18 6M6 6l12 12"
              />
            </svg>
          </button>
        </div>

        {/* Scrollable content */}
        <div class="overflow-y-auto p-6 pt-4">
          <div class="space-y-3 text-sm">
            <div class="flex justify-between">
              <span class="font-medium text-gray-500">Time:</span>
              <span class="text-gray-900">{formatTime(props.log.timestamp)}</span>
            </div>

            <div class="flex justify-between">
              <span class="font-medium text-gray-500">Operation:</span>
              <span class="font-semibold text-gray-900">{sshInfo().operation}</span>
            </div>

            <div class="flex justify-between">
              <span class="font-medium text-gray-500">Target:</span>
              <span class="font-mono text-xs text-gray-900 break-all text-right max-w-[300px]">
                {sshInfo().target}
              </span>
            </div>

            <div>
              <span class="font-medium text-gray-500">Key: </span>
              <span class="text-gray-900">{props.log.key_name}</span>
            </div>

            <div>
              <span class="font-medium text-gray-500">Fingerprint:</span>
              <div class="mt-1 font-mono text-xs text-gray-700 break-all rounded bg-gray-50 p-2">
                {props.log.key_fingerprint}
              </div>
            </div>

            {/* Process Chain */}
            <Show when={props.log.process_chain && props.log.process_chain.length > 0}>
              <div>
                <span class="font-medium text-gray-500">Process Chain:</span>
                <div class="mt-2 space-y-1">
                  <For each={props.log.process_chain}>
                    {(proc, index) => (
                      <div class="flex items-start gap-2 rounded bg-gray-50 p-2 text-xs">
                        <span class="shrink-0 font-mono text-gray-400">{index() + 1}.</span>
                        <div class="min-w-0 flex-1">
                          <div class="flex items-center gap-2">
                            <span class="font-semibold text-gray-900">
                              {extractExeName(proc.exe)}
                            </span>
                            <span class="text-gray-400">PID: {proc.pid}</span>
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
                            <div class="mt-0.5 text-gray-400 break-all">{proc.exe}</div>
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
              <div class="flex justify-between">
                <span class="font-medium text-gray-500">Client:</span>
                <span class="text-gray-900" title={props.log.client_exe}>
                  {extractExeName(props.log.client_exe)} (PID: {props.log.client_pid})
                </span>
              </div>
            </Show>
          </div>
        </div>

        {/* Footer - fixed */}
        <div class="p-6 pt-0">
          <button
            type="button"
            class="w-full rounded-lg bg-gray-100 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-200"
            onClick={props.onClose}
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

// --- Log Table ---

export function LogTable(props: LogTableProps) {
  const [selectedLog, setSelectedLog] = createSignal<AccessLogEntry | null>(null);

  return (
    <>
      <div class="overflow-hidden rounded-lg border border-gray-200 shadow-sm">
        <table class="w-full table-fixed divide-y divide-gray-200">
          <thead class="bg-gray-50">
            <tr>
              <th
                scope="col"
                class="w-[22%] px-3 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
              >
                Time
              </th>
              <th
                scope="col"
                class="w-[18%] px-3 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
              >
                Initiator
              </th>
              <th
                scope="col"
                class="w-[46%] px-3 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
              >
                Summary
              </th>
              <th
                scope="col"
                class="w-[14%] px-3 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
              >
                Status
              </th>
            </tr>
          </thead>
          <tbody class="divide-y divide-gray-200 bg-white">
            <Show
              when={props.logs.length > 0}
              fallback={
                <tr>
                  <td
                    colspan="4"
                    class="px-4 py-4 text-center text-sm text-gray-500"
                  >
                    No access logs
                  </td>
                </tr>
              }
            >
              <For each={props.logs}>
                {(log, index) => {
                  const sshInfo = parseSshCmdline(log.process_chain);
                  const initiator = getInitiator(log.process_chain, log.client_exe);

                  return (
                    <tr
                      class={`cursor-pointer transition-colors hover:bg-blue-50 ${
                        index() % 2 === 0 ? "bg-white" : "bg-gray-50"
                      }`}
                      onClick={() => setSelectedLog(log)}
                    >
                      <td class="px-3 py-3 text-sm text-gray-500 truncate">
                        {formatTime(log.timestamp)}
                      </td>

                      <td class="px-3 py-3 text-sm font-medium text-gray-900 truncate">
                        {initiator}
                      </td>

                      <td class="px-3 py-3 text-sm text-gray-700">
                        <div class="truncate">
                          <span class="font-medium">{sshInfo.operation}</span>
                          <Show when={sshInfo.target !== "unknown"}>
                            <span class="text-gray-400 mx-1">·</span>
                            <span class="font-mono text-gray-600">{sshInfo.target}</span>
                          </Show>
                        </div>
                      </td>

                      <td class="px-3 py-3 whitespace-nowrap text-sm">
                        <span
                          class={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${
                            log.approved
                              ? "bg-green-100 text-green-800"
                              : "bg-red-100 text-red-800"
                          }`}
                        >
                          {log.approved ? "Approved" : "Denied"}
                        </span>
                      </td>
                    </tr>
                  );
                }}
              </For>
            </Show>
          </tbody>
        </table>
      </div>

      {/* Detail Modal */}
      <Show when={selectedLog()}>
        {(log) => <LogDetailModal log={log()} onClose={() => setSelectedLog(null)} />}
      </Show>
    </>
  );
}
