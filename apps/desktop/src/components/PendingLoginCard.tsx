import { useState, type CSSProperties } from "react";
import { commands } from "../bindings";
import { C } from "../design";

interface PendingLoginCardProps {
    userCode: string;
    hostname: string;
    sourceRegion: string;
    requestedAt: number;
    onResolved: () => void;
}

function secondsAgo(unixSec: number): string {
    const delta = Math.max(0, Math.floor(Date.now() / 1000) - unixSec);
    if (delta < 60) return `${delta}s ago`;
    return `${Math.floor(delta / 60)}m ago`;
}

export function PendingLoginCard({
    userCode,
    hostname,
    sourceRegion,
    requestedAt,
    onResolved,
}: PendingLoginCardProps) {
    const [approveHovered, setApproveHovered] = useState(false);
    const [denyHovered, setDenyHovered] = useState(false);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const age = secondsAgo(requestedAt);

    const cardStyle: CSSProperties = {
        background: C.card,
        border: `1px solid ${C.border}`,
        borderRadius: 8,
        padding: "12px 16px",
        marginBottom: 8,
    };

    const approveStyle: CSSProperties = {
        background: approveHovered ? C.t1 : "transparent",
        color: approveHovered ? C.bg : C.t1,
        border: `1px solid ${C.border}`,
        borderRadius: 5,
        fontSize: 11,
        fontWeight: 600,
        padding: "4px 10px",
        cursor: busy ? "not-allowed" : "pointer",
        opacity: busy ? 0.5 : 1,
        transition: "background 120ms, color 120ms",
    };

    const denyStyle: CSSProperties = {
        background: "transparent",
        color: denyHovered ? C.error : C.t2,
        border: `1px solid ${denyHovered ? "rgba(255, 99, 99, 0.4)" : C.border}`,
        borderRadius: 5,
        fontSize: 11,
        fontWeight: 600,
        padding: "4px 10px",
        cursor: busy ? "not-allowed" : "pointer",
        opacity: busy ? 0.5 : 1,
        transition: "color 120ms, border-color 120ms",
    };

    async function handleApprove() {
        if (busy) return;
        setBusy(true);
        setError(null);
        try {
            const r = await commands.approveRemoteLogin(userCode);
            if (r.status === "ok") {
                onResolved();
            } else {
                setError(r.error);
            }
        } finally {
            setBusy(false);
        }
    }

    async function handleDeny() {
        if (busy) return;
        setBusy(true);
        setError(null);
        try {
            const r = await commands.denyRemoteLogin(userCode);
            if (r.status === "ok") {
                onResolved();
            } else {
                setError(r.error);
            }
        } finally {
            setBusy(false);
        }
    }

    return (
        <div style={cardStyle}>
            <div style={{ fontSize: 13, fontWeight: 600, color: C.t1, marginBottom: 4 }}>
                Are you signing in on <code style={{ fontFamily: "var(--font-mono)", background: C.card2, borderRadius: 3, padding: "1px 5px" }}>{hostname}</code> right now?
            </div>
            <div style={{ fontSize: 11, fontWeight: 500, color: C.t3, marginBottom: 10 }}>
                {sourceRegion && <span>{sourceRegion} · </span>}
                Code <code style={{ fontFamily: "var(--font-mono)", letterSpacing: "0.5px" }}>{userCode}</code> · Requested {age}
            </div>
            <div style={{ display: "flex", gap: 8 }}>
                <button
                    type="button"
                    style={approveStyle}
                    disabled={busy}
                    onClick={() => void handleApprove()}
                    onMouseEnter={() => setApproveHovered(true)}
                    onMouseLeave={() => setApproveHovered(false)}
                >
                    Approve
                </button>
                <button
                    type="button"
                    style={denyStyle}
                    disabled={busy}
                    onClick={() => void handleDeny()}
                    onMouseEnter={() => setDenyHovered(true)}
                    onMouseLeave={() => setDenyHovered(false)}
                >
                    Deny
                </button>
            </div>
            {error && (
                <div style={{ fontSize: 11, color: C.error, marginTop: 6 }}>
                    {error}
                </div>
            )}
        </div>
    );
}
