import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AddRelayDialog } from "./AddRelayDialog";
import { signIn } from "../lib/state/auth";

vi.mock("../lib/state/auth", () => ({
  signIn: vi.fn(),
}));

vi.mock("../bindings", () => ({
  commands: {
    pairWithToken: vi.fn(),
    listAuthProviders: vi.fn(),
  },
}));

describe("AddRelayDialog", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    const { commands } = await import("../bindings");
    vi.mocked(commands.listAuthProviders).mockResolvedValue(["github"]);
  });

  it("closes after browser sign-in launch succeeds", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();

    vi.mocked(signIn).mockResolvedValue(undefined);

    render(<AddRelayDialog onClose={onClose} />);

    await user.type(
      screen.getByPlaceholderText("https://api.example.com"),
      "https://api.cinchcli.com",
    );

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: /continue with github/i }),
      ).toBeInTheDocument();
    });

    await user.click(
      screen.getByRole("button", { name: /continue with github/i }),
    );

    expect(signIn).toHaveBeenCalledWith("https://api.cinchcli.com", "github");
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
