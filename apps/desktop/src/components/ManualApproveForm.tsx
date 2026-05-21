import { useState, type CSSProperties } from "react";
import { commands } from "../bindings";
import { C } from "../design";

interface ManualApproveFormProps {
    onApproved: () => void;
}

export function ManualApproveForm({ onApproved }: ManualApproveFormProps) {
    const [code, setCode] = useState("");
    const [error, setError] = useState<string | null>(null);
    const [busy, setBusy] = useState(false);

    const inputStyle: CSSProperties = {
        background: C.card,
        border: `1px solid ${error ? C.error : C.border}`,
        borderRadius: 6,
        color: C.t1,
        fontSize: 13,
        fontWeight: 500,
        fontFamily: "var(--font-mono)",
        letterSpacing: "0.5px",
        padding: "6px 10px",
        outline: "none",
        minWidth: 130,
        transition: "border-color 120ms",
    };

    const buttonStyle: CSSProperties = {
        background: "transparent",
        border: `1px solid ${C.border}`,
        borderRadius: 5,
        color: C.t1,
        fontSize: 11,
        fontWeight: 600,
        padding: "4px 10px",
        cursor: busy || !code.trim() ? "not-allowed" : "pointer",
        opacity: busy || !code.trim() ? 0.4 : 1,
        transition: "opacity 120ms",
    };

    async function handleApprove() {
        const trimmed = code.trim();
        if (!trimmed || busy) return;
        setError(null);
        setBusy(true);
        try {
            const r = await commands.approveRemoteLogin(trimmed);
            if (r.status === "ok") {
                setCode("");
                onApproved();
            } else {
                setError(r.error);
            }
        } finally {
            setBusy(false);
        }
    }

    return (
        <div style={{ borderTop: `1px solid ${C.border}`, paddingTop: 16, marginTop: 16 }}>
            <div style={{ fontSize: 13, fontWeight: 600, color: C.t1, marginBottom: 8 }}>
                Or enter a code manually
            </div>
            <div style={{ fontSize: 12, fontWeight: 500, color: C.t3, marginBottom: 10 }}>
                If you missed the notification, paste the code from your terminal here.
            </div>
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <input
                    type="text"
                    value={code}
                    onChange={(e) => { setCode(e.target.value); setError(null); }}
                    onKeyDown={(e) => { if (e.key === "Enter") void handleApprove(); }}
                    placeholder="ABCD-1234"
                    style={inputStyle}
                    disabled={busy}
                    aria-label="Device code"
                />
                <button
                    type="button"
                    style={buttonStyle}
                    disabled={busy || !code.trim()}
                    onClick={() => void handleApprove()}
                >
                    Approve
                </button>
            </div>
            {error && (
                <div style={{ fontSize: 12, fontWeight: 500, color: C.error, marginTop: 6 }}>
                    {error}
                </div>
            )}
        </div>
    );
}
