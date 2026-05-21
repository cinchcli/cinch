// Returns a layout-agnostic key identifier for matching shortcuts.
//
// Uses `e.code` (physical key position) for letters and digits so bindings
// fire under Korean IME (ㅓ on KeyJ, ㅏ on KeyK) and non-QWERTY layouts.
// Falls back to `e.key` for named keys (Escape, ArrowUp, Tab, /, ?, ...)
// and for test environments where `e.code` is not populated.
//
// Letters are normalized to uppercase A-Z so callers can match against "J".
export function physicalKey(e: Pick<KeyboardEvent, "code" | "key">): string {
  const code = e.code;
  if (code && /^Key[A-Z]$/.test(code)) return code.slice(3);
  if (code && /^Digit[0-9]$/.test(code)) return code.slice(5);
  if (e.key.length === 1 && /[a-zA-Z]/.test(e.key)) return e.key.toUpperCase();
  return e.key;
}
