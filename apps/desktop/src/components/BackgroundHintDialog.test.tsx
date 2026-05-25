import { render, screen, act, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

const h = vi.hoisted(() => ({
  cb: null as null | (() => void),
  resolveBackgroundHint: vi.fn((_quit: boolean) => Promise.resolve(null)),
}));

vi.mock("../bindings", () => ({
  events: {
    backgroundHint: {
      listen: vi.fn((cb: () => void) => {
        h.cb = cb;
        return Promise.resolve(() => {});
      }),
    },
  },
  commands: {
    resolveBackgroundHint: h.resolveBackgroundHint,
  },
}));

import { BackgroundHintDialog } from "./BackgroundHintDialog";

describe("BackgroundHintDialog", () => {
  beforeEach(() => h.resolveBackgroundHint.mockClear());

  it("is hidden until the backgroundHint event fires", async () => {
    render(<BackgroundHintDialog />);
    await waitFor(() => expect(h.cb).not.toBeNull());
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("shows on event; 'Keep in menu bar' resolves quit=false and closes", async () => {
    render(<BackgroundHintDialog />);
    await waitFor(() => expect(h.cb).not.toBeNull());
    act(() => h.cb!());
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /keep in menu bar/i }));
    expect(h.resolveBackgroundHint).toHaveBeenCalledWith(false);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("'Quit Cinch' resolves quit=true", async () => {
    render(<BackgroundHintDialog />);
    await waitFor(() => expect(h.cb).not.toBeNull());
    act(() => h.cb!());
    fireEvent.click(screen.getByRole("button", { name: /quit cinch/i }));
    expect(h.resolveBackgroundHint).toHaveBeenCalledWith(true);
  });

  it("Esc resolves quit=false (safe default = keep)", async () => {
    render(<BackgroundHintDialog />);
    await waitFor(() => expect(h.cb).not.toBeNull());
    act(() => h.cb!());
    fireEvent.keyDown(window, { key: "Escape" });
    expect(h.resolveBackgroundHint).toHaveBeenCalledWith(false);
  });

  it("clicking the overlay resolves quit=false (safe default = keep)", async () => {
    render(<BackgroundHintDialog />);
    await waitFor(() => expect(h.cb).not.toBeNull());
    act(() => h.cb!());
    // The overlay is the presentation wrapper directly behind the dialog.
    const overlay = screen.getByRole("dialog").parentElement as HTMLElement;
    fireEvent.click(overlay);
    expect(h.resolveBackgroundHint).toHaveBeenCalledWith(false);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("clicking inside the dialog does not resolve (stopPropagation)", async () => {
    render(<BackgroundHintDialog />);
    await waitFor(() => expect(h.cb).not.toBeNull());
    act(() => h.cb!());
    fireEvent.click(screen.getByRole("dialog"));
    expect(h.resolveBackgroundHint).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});
