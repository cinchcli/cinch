import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AddSshMachineDialog } from "./AddSshMachineDialog";

const mocks = vi.hoisted(() => ({
  listen: vi.fn(),
  listSshHosts: vi.fn(),
  pairViaSsh: vi.fn(),
}));

vi.mock("../bindings", () => ({
  commands: {
    listSshHosts: mocks.listSshHosts,
    pairViaSsh: mocks.pairViaSsh,
  },
  events: {
    sshPairMarkerFound: {
      listen: mocks.listen,
    },
  },
}));

describe("AddSshMachineDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.listen.mockResolvedValue(() => {});
    mocks.listSshHosts.mockResolvedValue({
      status: "ok",
      data: ["oci_atlas_1", "jgopi"],
    });
  });

  it("loads SSH config aliases into the target suggestions", async () => {
    render(
      <AddSshMachineDialog onClose={vi.fn()} onShowToast={vi.fn()} />,
    );

    await waitFor(() => {
      expect(mocks.listSshHosts).toHaveBeenCalledTimes(1);
    });

    expect(screen.getByRole("button", { name: "oci_atlas_1" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "jgopi" })).toBeInTheDocument();
  });
});
