import type { LucideIcon } from "lucide-react"
import {
  User,
  Lightbulb,
  HelpCircle,
  FileText,
  Target,
  TrendingUp,
  BookOpen,
  Calendar,
  Hash,
  // DEVWIKI: icons for the enterprise page types (P0/P1).
  Scale,
  ListChecks,
  Workflow,
  GraduationCap,
  Boxes,
  GitCompare,
  Layers,
} from "lucide-react"

export interface WikiTypeStyle {
  /** Display label for chips and tooltips. Capitalized. */
  label: string
  /** Lucide icon component for the type. */
  icon: LucideIcon
  /**
   * Tailwind classes for a colored chip (background + text). Includes
   * dark-mode variants so the chip stays readable on either theme.
   */
  chipClass: string
  /**
   * Tailwind class for the type's accent dot or border (without the
   * 15% opacity used by chips). Useful for connector lines.
   */
  dotClass: string
}

export const WIKI_TYPE_STYLES: Record<string, WikiTypeStyle> = {
  entity: {
    label: "Entity",
    icon: User,
    chipClass: "bg-blue-500/15 text-blue-700 dark:text-blue-300",
    dotClass: "bg-blue-500",
  },
  concept: {
    label: "Concept",
    icon: Lightbulb,
    chipClass: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300",
    dotClass: "bg-emerald-500",
  },
  query: {
    label: "Query",
    icon: HelpCircle,
    chipClass: "bg-amber-500/15 text-amber-700 dark:text-amber-300",
    dotClass: "bg-amber-500",
  },
  source: {
    label: "Source",
    icon: FileText,
    chipClass: "bg-slate-500/15 text-slate-700 dark:text-slate-300",
    dotClass: "bg-slate-500",
  },
  thesis: {
    label: "Thesis",
    icon: Target,
    chipClass: "bg-rose-500/15 text-rose-700 dark:text-rose-300",
    dotClass: "bg-rose-500",
  },
  finding: {
    label: "Finding",
    icon: TrendingUp,
    chipClass: "bg-purple-500/15 text-purple-700 dark:text-purple-300",
    dotClass: "bg-purple-500",
  },
  methodology: {
    label: "Methodology",
    icon: BookOpen,
    chipClass: "bg-teal-500/15 text-teal-700 dark:text-teal-300",
    dotClass: "bg-teal-500",
  },
  event: {
    label: "Event",
    icon: Calendar,
    chipClass: "bg-cyan-500/15 text-cyan-700 dark:text-cyan-300",
    dotClass: "bg-cyan-500",
  },
  overview: {
    label: "Overview",
    icon: BookOpen,
    chipClass: "bg-indigo-500/15 text-indigo-700 dark:text-indigo-300",
    dotClass: "bg-indigo-500",
  },
  // ── DEVWIKI: enterprise + remaining native page types ──────────────────
  // These were falling back to the generic "Page" chip; give each a
  // distinct icon + colour (colours chosen to not collide with the above).
  decision: {
    label: "Decision",
    icon: Scale, // 取舍/天平 — a decision record / ADR
    chipClass: "bg-orange-500/15 text-orange-700 dark:text-orange-300",
    dotClass: "bg-orange-500",
  },
  playbook: {
    label: "Playbook",
    icon: ListChecks, // actionable step checklist
    chipClass: "bg-sky-500/15 text-sky-700 dark:text-sky-300",
    dotClass: "bg-sky-500",
  },
  solution: {
    label: "Solution",
    icon: Workflow, // cross-domain solution hub
    chipClass: "bg-violet-500/15 text-violet-700 dark:text-violet-300",
    dotClass: "bg-violet-500",
  },
  learning: {
    label: "Learning",
    icon: GraduationCap, // experience capture
    chipClass: "bg-lime-500/15 text-lime-700 dark:text-lime-300",
    dotClass: "bg-lime-500",
  },
  "bc-readme": {
    label: "Meta",
    icon: Boxes, // bounded-context / knowledge-domain overview
    chipClass: "bg-fuchsia-500/15 text-fuchsia-700 dark:text-fuchsia-300",
    dotClass: "bg-fuchsia-500",
  },
  // Alias: some pages may carry type: meta instead of bc-readme.
  meta: {
    label: "Meta",
    icon: Boxes,
    chipClass: "bg-fuchsia-500/15 text-fuchsia-700 dark:text-fuchsia-300",
    dotClass: "bg-fuchsia-500",
  },
  comparison: {
    label: "Comparison",
    icon: GitCompare,
    chipClass: "bg-pink-500/15 text-pink-700 dark:text-pink-300",
    dotClass: "bg-pink-500",
  },
  synthesis: {
    label: "Synthesis",
    icon: Layers,
    chipClass: "bg-yellow-500/15 text-yellow-700 dark:text-yellow-300",
    dotClass: "bg-yellow-500",
  },
}

export const FALLBACK_TYPE_STYLE: WikiTypeStyle = {
  label: "Page",
  icon: Hash,
  chipClass: "bg-muted text-muted-foreground",
  dotClass: "bg-muted-foreground/60",
}

export function getWikiTypeStyle(type: string | null | undefined): WikiTypeStyle {
  if (!type) return FALLBACK_TYPE_STYLE
  const key = type.trim().toLowerCase()
  return WIKI_TYPE_STYLES[key] ?? FALLBACK_TYPE_STYLE
}
