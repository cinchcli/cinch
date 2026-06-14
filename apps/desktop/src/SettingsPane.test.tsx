import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

// Stub ResizeObserver for jsdom (RetentionSlider uses it)
if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
}

import SettingsPane from "./SettingsPane";

// Track invoke calls for assertion
const invoke = vi.fn();

// Mock @tauri-apps/api/core
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

// Mock @tauri-apps/api/event (required now that SettingsPane subscribes to
// events.deviceCodePending on mount)
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  once: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(() => Promise.resolve()),
}));

// Mock @tauri-apps/plugin-global-shortcut
const mockRegister = vi.fn(() => Promise.resolve());
const mockUnregister = vi.fn(() => Promise.resolve());
const mockIsRegistered = vi.fn(() => Promise.resolve(true));

vi.mock("@tauri-apps/plugin-global-shortcut", () => ({
  register: (...args: unknown[]) => mockRegister(...args),
  unregister: (...args: unknown[]) => mockUnregister(...args),
  isRegistered: (...args: unknown[]) => mockIsRegistered(...args),
}));

// Mock @tauri-apps/api/window
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    show: vi.fn(() => Promise.resolve()),
    setFocus: vi.fn(() => Promise.resolve()),
  }),
}));

describe("SettingsPane", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    // Default mocks: retention config loads, global shortcut loads
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "get_retention_config") {
        return Promise.resolve({ local_days: 30, remote_days: 30 });
      }
      if (cmd === "get_global_shortcut") {
        return Promise.resolve("CmdOrCtrl+Shift+V");
      }
      if (cmd === "set_global_shortcut") {
        return Promise.resolve();
      }
      if (cmd === "get_send_shortcut") {
        return Promise.resolve(null);
      }
      if (cmd === "set_send_shortcut") {
        return Promise.resolve(null);
      }
      if (cmd === "get_action_shortcuts" || cmd === "reset_action_shortcuts") {
        return Promise.resolve({
          edit: "CmdOrCtrl+E",
          copy: "Enter",
          pin: "CmdOrCtrl+P",
          send: "CmdOrCtrl+Enter",
        });
      }
      if (cmd === "set_action_shortcuts") {
        return Promise.resolve();
      }
      if (cmd === "get_user_profile") {
        return Promise.resolve({
          email: "user@example.com",
          identity_provider: "google",
          user_id: "user_123",
        });
      }
      return Promise.resolve();
    });
  });

  describe("Global shortcut field", () => {
    it("renders the global shortcut input with label", async () => {
      render(<SettingsPane onClose={() => {}} clipCount={0} />);
      fireEvent.click(screen.getByText("Keyboard"));
      const input = await screen.findByLabelText("Global launch shortcut");
      expect(input).toBeInTheDocument();
      expect(screen.getByText(/Press a new key combination/i)).toBeInTheDocument();
    });

    it("displays the shortcut in macOS symbol format", async () => {
      render(<SettingsPane onClose={() => {}} clipCount={0} />);
      fireEvent.click(screen.getByText("Keyboard"));
      const input = await screen.findByLabelText("Global launch shortcut");
      // CmdOrCtrl+Shift+V should display as command+shift+V symbols
      await waitFor(() => {
        expect(input).toHaveValue("\u2318\u21E7V");
      });
    });

    it("shows error for key press without modifier", async () => {
      render(<SettingsPane onClose={() => {}} clipCount={0} />);
      fireEvent.click(screen.getByText("Keyboard"));
      const input = await screen.findByLabelText("Global launch shortcut");
      fireEvent.keyDown(input, {
        key: "v",
        metaKey: false,
        ctrlKey: false,
        shiftKey: false,
        altKey: false,
      });
      expect(
        await screen.findByText("Shortcut must include a modifier key")
      ).toBeInTheDocument();
    });

    it("ignores modifier-only presses", async () => {
      render(<SettingsPane onClose={() => {}} clipCount={0} />);
      fireEvent.click(screen.getByText("Keyboard"));
      const input = await screen.findByLabelText("Global launch shortcut");
      fireEvent.keyDown(input, {
        key: "Meta",
        metaKey: true,
        ctrlKey: false,
        shiftKey: false,
        altKey: false,
      });
      // Should not call set_global_shortcut for modifier-only press
      expect(invoke).not.toHaveBeenCalledWith(
        "set_global_shortcut",
        expect.anything()
      );
    });

    it("captures modifier+key combination and persists", async () => {
      render(<SettingsPane onClose={() => {}} clipCount={0} />);
      fireEvent.click(screen.getByText("Keyboard"));
      const input = await screen.findByLabelText("Global launch shortcut");
      fireEvent.keyDown(input, {
        key: "b",
        metaKey: true,
        ctrlKey: false,
        shiftKey: true,
        altKey: false,
      });
      await waitFor(() => {
        expect(invoke).toHaveBeenCalledWith("set_global_shortcut", {
          shortcut: "CmdOrCtrl+Shift+B",
        });
      });
    });

    it("shows error when set_global_shortcut fails", async () => {
      invoke.mockImplementation((cmd: string) => {
        if (cmd === "get_retention_config") {
          return Promise.resolve({ local_days: 30, remote_days: 30 });
        }
        if (cmd === "get_global_shortcut") {
          return Promise.resolve("CmdOrCtrl+Shift+V");
        }
        if (cmd === "set_global_shortcut") {
          return Promise.reject(new Error("invalid"));
        }
        return Promise.resolve();
      });

      render(<SettingsPane onClose={() => {}} clipCount={0} />);
      fireEvent.click(screen.getByText("Keyboard"));
      const input = await screen.findByLabelText("Global launch shortcut");
      fireEvent.keyDown(input, {
        key: "x",
        metaKey: true,
        ctrlKey: false,
        shiftKey: false,
        altKey: false,
      });
      expect(await screen.findByText("Invalid shortcut")).toBeInTheDocument();
    });
  });

  describe("Send shortcut field", () => {
    it("shows 'Off' in the send shortcut input when getSendShortcut resolves to null", async () => {
      render(<SettingsPane onClose={() => {}} clipCount={0} />);
      fireEvent.click(screen.getByText("Keyboard"));
      const input = await screen.findByLabelText("Send clipboard shortcut");
      await waitFor(() => {
        expect(input).toHaveValue("Off");
      });
    });
  });

  describe("Clip action shortcuts", () => {
    it("captures a free chord, persists it, and propagates to the parent", async () => {
      const onChange = vi.fn();
      render(
        <SettingsPane onClose={() => {}} clipCount={0} onActionShortcutsChange={onChange} />,
      );
      fireEvent.click(screen.getByText("Keyboard"));
      const input = await screen.findByLabelText("Edit clip shortcut");
      // ⌘B is free (no fixed handler owns B, not a default action binding).
      fireEvent.keyDown(input, { code: "KeyB", key: "b", metaKey: true });

      const expected = {
        edit: "CmdOrCtrl+B",
        copy: "Enter",
        pin: "CmdOrCtrl+P",
        send: "CmdOrCtrl+Enter",
      };
      await waitFor(() => {
        expect(invoke).toHaveBeenCalledWith("set_action_shortcuts", { shortcuts: expected });
      });
      expect(onChange).toHaveBeenCalledWith(expected);
    });

    it("blocks a conflicting binding, shows an inline error, and persists nothing", async () => {
      const onChange = vi.fn();
      render(
        <SettingsPane onClose={() => {}} clipCount={0} onActionShortcutsChange={onChange} />,
      );
      fireEvent.click(screen.getByText("Keyboard"));
      const input = await screen.findByLabelText("Edit clip shortcut");
      // Bare Enter is Copy's default binding → duplicate-within-four conflict.
      fireEvent.keyDown(input, { code: "Enter", key: "Enter" });

      expect(await screen.findByText(/already used by Copy clip/i)).toBeInTheDocument();
      expect(invoke).not.toHaveBeenCalledWith("set_action_shortcuts", expect.anything());
      expect(onChange).not.toHaveBeenCalled();
    });

    it("ignores a modifier-only key press", async () => {
      render(<SettingsPane onClose={() => {}} clipCount={0} />);
      fireEvent.click(screen.getByText("Keyboard"));
      const input = await screen.findByLabelText("Edit clip shortcut");
      fireEvent.keyDown(input, { key: "Meta", metaKey: true });

      expect(invoke).not.toHaveBeenCalledWith("set_action_shortcuts", expect.anything());
    });

    it("reset to defaults calls reset_action_shortcuts and propagates the defaults", async () => {
      const onChange = vi.fn();
      render(
        <SettingsPane onClose={() => {}} clipCount={0} onActionShortcutsChange={onChange} />,
      );
      fireEvent.click(screen.getByText("Keyboard"));
      const resetBtn = await screen.findByRole("button", { name: /Reset to defaults/i });
      fireEvent.click(resetBtn);

      await waitFor(() => {
        expect(invoke).toHaveBeenCalledWith("reset_action_shortcuts");
      });
      expect(onChange).toHaveBeenCalledWith({
        edit: "CmdOrCtrl+E",
        copy: "Enter",
        pin: "CmdOrCtrl+P",
        send: "CmdOrCtrl+Enter",
      });
    });
  });

  describe("Privacy trust block", () => {
    it("renders the self-host guide link in the Privacy tab, opening in a new tab", async () => {
      render(<SettingsPane onClose={() => {}} clipCount={0} />);

      fireEvent.click(screen.getByText("Privacy"));
      const link = (await screen.findByRole("link", {
        name: /Read the self-host guide/i,
      })) as HTMLAnchorElement;

      expect(link).toBeInTheDocument();
      expect(link).toHaveAttribute("href", "https://cinchcli.com/docs/self-hosting");
      expect(link).toHaveAttribute("target", "_blank");
      expect(link).toHaveAttribute("rel", expect.stringContaining("noopener"));
    });
  });

  describe("Clip filters", () => {
    it("does not expose editable filter rules", async () => {
      render(<SettingsPane onClose={() => {}} clipCount={0} />);

      fireEvent.click(screen.getByText("Privacy"));
      await screen.findByText("Local retention");

      expect(screen.queryByText("Clip filters")).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Save filter rules" })).not.toBeInTheDocument();
    });
  });

  describe("Notifications toggle", () => {
    it("shows the remote login notification checkbox in the General tab, checked by default", async () => {
      localStorage.removeItem("cinch.notify_on_remote_login");
      render(<SettingsPane onClose={() => {}} clipCount={0} />);

      const checkbox = (await screen.findByLabelText(
        /remote login is pending approval/i
      )) as HTMLInputElement;
      expect(checkbox).toBeInTheDocument();
      expect(checkbox.checked).toBe(true);
    });

    it("toggles the notification setting and persists to localStorage", async () => {
      localStorage.setItem("cinch.notify_on_remote_login", "1");
      render(<SettingsPane onClose={() => {}} clipCount={0} />);

      const checkbox = (await screen.findByLabelText(
        /remote login is pending approval/i
      )) as HTMLInputElement;
      expect(checkbox.checked).toBe(true);

      fireEvent.click(checkbox);

      expect(checkbox.checked).toBe(false);
      expect(localStorage.getItem("cinch.notify_on_remote_login")).toBe("0");
    });

    it("reads an existing false value from localStorage on mount", async () => {
      localStorage.setItem("cinch.notify_on_remote_login", "0");
      render(<SettingsPane onClose={() => {}} clipCount={0} />);

      const checkbox = (await screen.findByLabelText(
        /remote login is pending approval/i
      )) as HTMLInputElement;
      expect(checkbox.checked).toBe(false);
    });
  });
});
