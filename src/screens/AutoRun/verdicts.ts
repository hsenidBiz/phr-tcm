// The words a human can pick as a verdict, and how each one is coloured
// once picked - shared by the supervised pane (RunPane) and the review
// screen (RunReview) so a "Failed" button looks the same wherever it is
// pressed. One copy: a second one is how the two panes quietly drift.

export const VERDICTS = ["Passed", "Failed", "Blocked"] as const;

export const verdictTone: Record<string, string> = {
  Passed: "bg-success/20 text-success",
  Failed: "bg-danger/20 text-danger",
  Blocked: "bg-warning/20 text-warning",
};
