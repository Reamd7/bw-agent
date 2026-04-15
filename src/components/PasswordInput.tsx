import { createSignal, Show } from "solid-js";

interface PasswordInputProps {
  value: string;
  onInput: (value: string) => void;
  error?: string;
  disabled?: boolean;
  placeholder?: string;
}

export default function PasswordInput(props: PasswordInputProps) {
  const [showPassword, setShowPassword] = createSignal(false);

  return (
    <div class="w-full flex flex-col gap-1">
      <div class="relative w-full">
        <input
          type={showPassword() ? "text" : "password"}
          value={props.value}
          onInput={(e) => props.onInput(e.currentTarget.value)}
          disabled={props.disabled}
          placeholder={props.placeholder || "Password"}
          class="w-full px-4 py-2 bg-zinc-900 border border-zinc-700 rounded-md text-zinc-100 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50 transition-all pr-10"
          autofocus
        />
        <button
          type="button"
          onClick={() => setShowPassword(!showPassword())}
          disabled={props.disabled}
          class="absolute right-2 top-1/2 -translate-y-1/2 p-1 text-zinc-400 hover:text-zinc-200 focus:outline-none disabled:opacity-50"
          aria-label={showPassword() ? "Hide password" : "Show password"}
        >
          <Show when={showPassword()} fallback={
            <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z" />
              <circle cx="12" cy="12" r="3" />
            </svg>
          }>
            <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M9.88 9.88a3 3 0 1 0 4.24 4.24" />
              <path d="M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68" />
              <path d="M6.61 6.61A13.526 13.526 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61" />
              <line x1="2" x2="22" y1="2" y2="22" />
            </svg>
          </Show>
        </button>
      </div>
      <Show when={props.error}>
        <span class="text-sm text-red-500">{props.error}</span>
      </Show>
    </div>
  );
}
