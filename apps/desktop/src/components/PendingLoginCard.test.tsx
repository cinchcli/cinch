import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve()) }));
vi.mock("@tauri-apps/api/event", () => ({
    listen: vi.fn(() => Promise.resolve(() => {})),
    once: vi.fn(() => Promise.resolve(() => {})),
    emit: vi.fn(() => Promise.resolve()),
}));

vi.mock("../bindings", () => ({
    commands: {
        approveRemoteLogin: vi.fn().mockResolvedValue({ status: "ok", data: null }),
        denyRemoteLogin: vi.fn().mockResolvedValue({ status: "ok", data: null }),
    },
}));

import { PendingLoginCard } from "./PendingLoginCard";
import { commands } from "../bindings";

const baseProps = {
    userCode: "ABCD-1234",
    hostname: "dev-box-3",
    sourceRegion: "us-west",
    requestedAt: Math.floor(Date.now() / 1000),
};

describe("PendingLoginCard", () => {
    beforeEach(() => {
        vi.clearAllMocks();
    });

    it("renders hostname and user code", () => {
        render(<PendingLoginCard {...baseProps} onResolved={() => {}} />);
        expect(screen.getByText("dev-box-3")).toBeInTheDocument();
        expect(screen.getByText("ABCD-1234")).toBeInTheDocument();
    });

    it("renders source region when provided", () => {
        render(<PendingLoginCard {...baseProps} onResolved={() => {}} />);
        expect(screen.getByText(/us-west/)).toBeInTheDocument();
    });

    it("calls approveRemoteLogin and onResolved when Approve is clicked", async () => {
        const onResolved = vi.fn();
        render(<PendingLoginCard {...baseProps} onResolved={onResolved} />);

        fireEvent.click(screen.getByRole("button", { name: "Approve" }));

        await waitFor(() => {
            expect(commands.approveRemoteLogin).toHaveBeenCalledWith("ABCD-1234");
            expect(onResolved).toHaveBeenCalledTimes(1);
        });
    });

    it("calls denyRemoteLogin and onResolved when Deny is clicked", async () => {
        const onResolved = vi.fn();
        render(<PendingLoginCard {...baseProps} onResolved={onResolved} />);

        fireEvent.click(screen.getByRole("button", { name: "Deny" }));

        await waitFor(() => {
            expect(commands.denyRemoteLogin).toHaveBeenCalledWith("ABCD-1234");
            expect(onResolved).toHaveBeenCalledTimes(1);
        });
    });

    it("does not render source region region text when sourceRegion is empty", () => {
        render(
            <PendingLoginCard
                {...baseProps}
                sourceRegion=""
                onResolved={() => {}}
            />,
        );
        expect(screen.queryByText(/us-west/)).not.toBeInTheDocument();
    });

    it("shows error when approve fails and does not call onResolved", async () => {
        const onResolved = vi.fn();
        (commands.approveRemoteLogin as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
            status: "error",
            error: "relay unreachable",
        });
        render(<PendingLoginCard {...baseProps} onResolved={onResolved} />);

        fireEvent.click(screen.getByRole("button", { name: "Approve" }));

        await screen.findByText(/relay unreachable/);
        expect(onResolved).not.toHaveBeenCalled();
    });

    it("shows error when deny fails and does not call onResolved", async () => {
        const onResolved = vi.fn();
        (commands.denyRemoteLogin as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
            status: "error",
            error: "network timeout",
        });
        render(<PendingLoginCard {...baseProps} onResolved={onResolved} />);

        fireEvent.click(screen.getByRole("button", { name: "Deny" }));

        await screen.findByText(/network timeout/);
        expect(onResolved).not.toHaveBeenCalled();
    });
});
