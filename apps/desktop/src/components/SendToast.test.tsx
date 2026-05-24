import { render, screen, act, waitFor } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";

const h = vi.hoisted(() => ({
  cb: null as null | ((e: { payload: boolean }) => void),
}));

vi.mock("../bindings", () => ({
  events: {
    clipSent: {
      listen: vi.fn((cb: (e: { payload: boolean }) => void) => {
        h.cb = cb;
        return Promise.resolve(() => {});
      }),
    },
  },
}));

import { SendToast } from "./SendToast";

describe("SendToast", () => {
  it("shows a confirmation when a clip is sent", async () => {
    render(<SendToast />);
    await waitFor(() => expect(h.cb).not.toBeNull());
    act(() => {
      h.cb!({ payload: true });
    });
    expect(screen.getByText(/sent to your devices/i)).toBeInTheDocument();
  });

  it("shows nothing-to-send when payload is false", async () => {
    render(<SendToast />);
    await waitFor(() => expect(h.cb).not.toBeNull());
    act(() => {
      h.cb!({ payload: false });
    });
    expect(screen.getByText(/nothing to send/i)).toBeInTheDocument();
  });

  it("is not visible before any event fires", () => {
    render(<SendToast />);
    expect(screen.queryByText(/sent to your devices/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/nothing to send/i)).not.toBeInTheDocument();
  });
});
