// Centralised decision-state styling so /audit, /review, and any future
// surfaces stay consistent. Colour map per the M12 spec:
//
//   auto-green     → green   (auto-cleared, no review needed)
//   auto-block     → red     (exact name/alias match, no review needed)
//   pending-review → purple  (yellow TLP, awaits review)
//   pending-block  → red     (red TLP near-match, awaits review)
//   cleared        → green   (reviewed, decided OK)
//   blocked        → red     (reviewed, decided NO)
//
// Each row carries dot/border/text/bg classes and a short label. The
// dot+label combo is what makes this a11y-safe — no decision is
// distinguished by colour alone.

export type DecisionStyle = {
  label: string;
  dot: string;
  text: string;
  bg: string;
  border: string;
};

const PURPLE: DecisionStyle = {
  label: "pending review",
  dot: "bg-purple-500",
  text: "text-purple-600 dark:text-purple-400",
  bg: "bg-purple-500/10",
  border: "border-purple-500/40",
};

const GREEN: DecisionStyle = {
  label: "cleared",
  dot: "bg-tlp-green",
  text: "text-tlp-green",
  bg: "bg-tlp-green/10",
  border: "border-tlp-green/40",
};

const RED: DecisionStyle = {
  label: "blocked",
  dot: "bg-tlp-red",
  text: "text-tlp-red",
  bg: "bg-tlp-red/10",
  border: "border-tlp-red/40",
};

export function decisionStyle(decision: string): DecisionStyle {
  switch (decision) {
    case "auto-green":
      return { ...GREEN, label: "auto-cleared" };
    case "cleared":
      return { ...GREEN, label: "cleared" };
    case "auto-block":
      return { ...RED, label: "auto-blocked" };
    case "blocked":
      return { ...RED, label: "blocked" };
    case "pending-block":
      return { ...RED, label: "pending block" };
    case "pending-review":
      return { ...PURPLE, label: "pending review" };
    default:
      return { ...PURPLE, label: decision };
  }
}
