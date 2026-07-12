import type { Step } from "../bindings";
import { Button } from "./ui/button";
import { Input } from "./ui/input";

/** The step grid used by Manual Entry AND the case editor: numbered rows of
 * action/expected with reorder, remove and Add Step - one interaction model
 * everywhere (the v1 steps table). */
export default function StepsEditor({
  steps,
  onChange,
}: {
  steps: Step[];
  onChange: (steps: Step[]) => void;
}) {
  const setStep = (i: number, key: "action" | "expected", value: string) =>
    onChange(steps.map((s, j) => (j === i ? { ...s, [key]: value } : s)));

  const move = (i: number, delta: -1 | 1) => {
    const j = i + delta;
    if (j < 0 || j >= steps.length) return;
    const next = [...steps];
    [next[i], next[j]] = [next[j], next[i]];
    onChange(next);
  };

  return (
    <div className="space-y-1">
      {steps.length === 0 && (
        <p className="text-xs text-faint">No steps yet - add the first one below.</p>
      )}
      {steps.map((s, i) => (
        <div key={i} className="flex items-center gap-1">
          <span className="id-mono w-5 text-right text-xs text-faint">{i + 1}</span>
          <Input
            aria-label={`Step ${i + 1} action`}
            className="flex-1 px-2 py-1.5 text-xs"
            placeholder="Action"
            value={s.action}
            onChange={(e) => setStep(i, "action", e.target.value)}
          />
          <Input
            aria-label={`Step ${i + 1} expected`}
            className="flex-1 px-2 py-1.5 text-xs"
            placeholder="Expected result"
            value={s.expected}
            onChange={(e) => setStep(i, "expected", e.target.value)}
          />
          <button className="px-1 text-xs text-faint hover:text-text" title="Move up" onClick={() => move(i, -1)}>
            ↑
          </button>
          <button className="px-1 text-xs text-faint hover:text-text" title="Move down" onClick={() => move(i, 1)}>
            ↓
          </button>
          <button
            className="px-1 text-xs text-faint hover:text-danger"
            title="Remove step"
            onClick={() => onChange(steps.filter((_, j) => j !== i))}
          >
            ✕
          </button>
        </div>
      ))}
      <Button
        variant="ghost"
        size="sm"
        onClick={() => onChange([...steps, { action: "", expected: "" }])}
      >
        + Add Step
      </Button>
    </div>
  );
}
