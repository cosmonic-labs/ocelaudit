// Compact coloured tag chip. Used on search/review/audit hits to
// surface CSL category + program + nationality + entity-type. Each
// `kind` picks a distinct palette so a hit's chip strip is at-a-glance
// readable. Non-essential decoration — every chip carries text too,
// so it stays a11y-safe.

type Kind = "source" | "entity" | "program" | "nationality" | "neutral";

interface Props {
  kind: Kind;
  /// Used to look up the family palette for `source` kind, ignored for others.
  source_code?: string;
  href?: string;
  title?: string;
  children: preact.ComponentChildren;
}

export function Tag({ kind, source_code, href, title, children }: Props) {
  const cls = palette(kind, source_code);
  const base =
    "inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-mono uppercase tracking-wider";
  if (href) {
    return (
      <a href={href} target="_blank" rel="noreferrer noopener" title={title} class={`${base} ${cls} hover:underline`}>
        {children}
      </a>
    );
  }
  return (
    <span title={title} class={`${base} ${cls}`}>
      {children}
    </span>
  );
}

function palette(kind: Kind, source_code?: string): string {
  switch (kind) {
    case "source":
      return sourcePalette(source_code ?? "");
    case "entity":
      return "bg-indigo-500/10 text-indigo-700 dark:text-indigo-300";
    case "program":
      return "bg-amber-500/10 text-amber-700 dark:text-amber-300";
    case "nationality":
      return "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
    case "neutral":
    default:
      return "bg-neutral-200/60 text-neutral-700 dark:bg-neutral-700/40 dark:text-neutral-200";
  }
}

// Family-coded palette: OFAC family is rose/red; export-control family
// (BIS / DTC) is sky/blue; everything else neutral. The actual short
// code is always rendered as text, so colour is just a quick visual.
function sourcePalette(code: string): string {
  if (["SDN", "NS-MBS", "NS-ISA", "FSE", "SSI", "CAPTA", "PLC"].includes(code)) {
    return "bg-rose-500/10 text-rose-700 dark:text-rose-300";
  }
  if (["EL", "UVL", "DPL", "ITAR-DPL"].includes(code)) {
    return "bg-sky-500/10 text-sky-700 dark:text-sky-300";
  }
  return "bg-neutral-200/60 text-neutral-700 dark:bg-neutral-700/40 dark:text-neutral-200";
}
