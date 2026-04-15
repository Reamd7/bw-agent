import { createSignal, onMount } from "solid-js";

interface TotpInputProps {
  onSubmit: (code: string) => void;
  disabled?: boolean;
}

export default function TotpInput(props: TotpInputProps) {
  const [code, setCode] = createSignal("");
  let inputRef: HTMLInputElement | undefined;

  onMount(() => {
    inputRef?.focus();
  });

  const handleInput = (e: Event) => {
    const target = e.currentTarget as HTMLInputElement;
    // Only allow digits
    const value = target.value.replace(/\D/g, "").slice(0, 6);
    setCode(value);
    target.value = value;

    if (value.length === 6) {
      props.onSubmit(value);
    }
  };

  return (
    <div class="w-full flex flex-col gap-2 items-center">
      <input
        ref={inputRef}
        type="text"
        inputmode="numeric"
        autocomplete="one-time-code"
        pattern="\d{6}"
        maxlength="6"
        value={code()}
        onInput={handleInput}
        disabled={props.disabled}
        placeholder="000000"
        class="w-48 text-center text-2xl tracking-[0.5em] font-mono px-4 py-3 bg-zinc-900 border border-zinc-700 rounded-md text-zinc-100 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50 transition-all"
      />
      <p class="text-sm text-zinc-400">Enter 6-digit authenticator code</p>
    </div>
  );
}
