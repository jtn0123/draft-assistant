import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it } from "vitest";
import { ChatControls } from "./ChatControls";

function Picker() {
  const [model, setModel] = useState("Opus 5");
  return (
    <ChatControls
      settings={null}
      models={["Opus 5", "GPT-6 Astra", "GPT-5.6 Sol"]}
      model={model}
      onModel={setModel}
      efforts={["High"]}
      effort="High"
      onEffort={() => undefined}
    />
  );
}

it("expands the model choices and collapses with focus on the selected model", async () => {
  const user = userEvent.setup();
  render(<Picker />);
  const toggle = screen.getByRole("button", { name: "Model: Opus 5" });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("group", { name: "Model" })).not.toBeInTheDocument();
  await user.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "true");
  await user.click(screen.getByRole("button", { name: "GPT-6 Astra" }));
  expect(toggle).toHaveAccessibleName("Model: GPT-6 Astra");
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(toggle).toHaveFocus();
  await user.click(toggle);
  await user.tab();
  await user.keyboard("{Escape}");
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(toggle).toHaveFocus();
});
