import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import {
  defaultRealtimeServiceFormValue,
  RealtimeServiceForm,
  type RealtimeServiceFormValue,
} from "./realtime-service-form";

vi.mock("@/components/shared/resource-selectors", () => ({
  ProjectSelector: () => <div>Project selector</div>,
}));

function FormHarness() {
  const [value, setValue] = useState<RealtimeServiceFormValue>(
    defaultRealtimeServiceFormValue,
  );

  return (
    <RealtimeServiceForm
      value={value}
      onChange={setValue}
      onSubmit={(event) => event.preventDefault()}
    >
      <button type="submit">保存</button>
    </RealtimeServiceForm>
  );
}

describe("RealtimeServiceForm", () => {
  it("ルーム上限を100で初期化し、正の整数入力として編集できる", () => {
    render(<FormHarness />);

    const input = screen.getByRole("spinbutton", { name: "ルーム上限" });
    expect(input).toHaveValue(100);
    expect(input).toHaveAttribute("min", "1");
    expect(input).toHaveAttribute("step", "1");

    fireEvent.change(input, { target: { value: "250" } });
    expect(input).toHaveValue(250);

    fireEvent.change(input, { target: { value: "-4" } });
    expect(input).toHaveValue(1);
  });

  it("IPレート制限を既定値で初期化し、正の整数として編集できる", () => {
    render(<FormHarness />);

    const rps = screen.getByRole("spinbutton", { name: "RPS上限" });
    const burst = screen.getByRole("spinbutton", { name: "バースト上限" });
    expect(rps).toHaveValue(20);
    expect(rps).toHaveAttribute("max", "1000");
    expect(burst).toHaveValue(40);
    expect(burst).toHaveAttribute("max", "5000");

    fireEvent.change(rps, { target: { value: "75" } });
    fireEvent.change(burst, { target: { value: "150" } });
    expect(rps).toHaveValue(75);
    expect(burst).toHaveValue(150);
  });
});
