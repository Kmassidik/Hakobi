# Hakobi UI Style Prompt

Use this prompt as the canonical visual and interaction reference for every
Hakobi interface. Preserve real product behavior and honest states; never copy
mock data, simulated actions, unsupported options, or unsupported claims from a
reference implementation.

---

Design or modify this Hakobi interface as a focused, production-grade developer
tool, not a generic admin template. Use the supplied Hakobi new-app wizard as
the visual source of truth. Match its layout hierarchy, proportions, spacing,
typography, borders, control shapes, and restrained color usage before adding
anything new.

## Visual system

- Use Inter or Hakobi's existing sans font for UI text, Newsreader or Hakobi's
  display serif for page and panel headings, and JetBrains Mono or the existing
  monospace font for repositories, paths, ports, environment names, and other
  machine identifiers.
- Page background: neutral `#fafafa`/paper. Panels: white/surface. Primary text:
  zinc-950/ink. Secondary text: zinc-500/muted. Borders: zinc-200/line.
- Primary action and active state: Hakobi green `#165237`; hover `#11402b`.
  Use the existing lime brand mark only for identity, not as a general button
  color. Dangerous actions use red sparingly.
- Use rounded-lg controls and rounded-xl cards. Borders are thin and low
  contrast. Shadows are subtle: `0 1px 2px rgba(0,0,0,.03)` or the existing
  equivalent. Avoid heavy shadows, gradients, glass effects, and decoration.
- Body and control text is normally 13px. Helper text is 11–12px. Page headings
  are 30–34px display serif; panel headings are about 22px display serif.
- Prefer generous but compact developer-tool spacing: 24px panel body padding,
  16–20px field groups, 12–16px related controls, and 24px workspace columns.
- Every input gets a visible label, useful placeholder where appropriate, and
  concise helper text only when it adds information. Focus uses a green border
  and a soft three-pixel green ring. Never remove focus indication.

## Page composition

- Keep Hakobi's existing navigation and authentication shell.
- Use a compact white breadcrumb bar at the top of the main workspace. Keep the
  content centered at approximately `max-w-5xl`, with responsive horizontal
  padding and 28–36px vertical padding.
- Use a small green eyebrow, a compact display heading, and one restrained
  description. Do not inflate headings or add marketing copy.
- For a primary workflow with supporting live information, use a 12-column grid:
  eight columns for the main task and four for a sticky summary. Stack the
  summary below the task before the layout becomes cramped.
- Cards have distinct header, body, and footer regions. Footers use a subtle
  zinc-50 background and top border. Navigation and submission actions belong in
  one predictable footer row, not scattered between panels.

## Wizard pattern

- Present progress as one connected horizontal stepper card: two columns on
  small screens and four on desktop. Each step contains a circular number or
  completion badge, a title, a short subtitle, and a thin progress bar.
- Active steps use solid green; completed steps use a green check and completed
  bar; upcoming steps use an outlined neutral badge and empty bar.
- Each panel has a clear title, description, `Step N of 4`, its actual fields,
  and a unified footer. Back is secondary; Continue/Build is primary.
- Validate before advancing. Direct navigation cannot bypass earlier invalid
  steps. Preserve all values when moving backward or forward. Move focus to the
  active heading and expose `aria-current="step"`.
- The review is a structured definition list with machine identifiers in mono
  and explicit Edit actions that return to the relevant step.
- A sticky recipe summary mirrors real current values: step, name, repository,
  branch, host, runtime/port, allocation, environment count, and deployment
  policy. Empty values use an em dash or Not selected, never fabricated samples.
- Update summaries after user input and programmatic auto-detection. The summary
  is informational; it must not replace the final review or server validation.

## Application settings pattern

- Keep the guided wizard only for app creation. Existing apps use compact,
  deep-linkable General, Source, Build, Processes, and Environment & data tabs.
- Switching tabs preserves unsaved values. A shared sticky footer exposes dirty
  state, Discard, and Save changes. Native query links and POST remain usable
  without JavaScript, and validation reveals the tab containing the error.

## Build terminal pattern

- Place one inline Deployment section on the existing app page, not a modal,
  sidebar, or separate default terminal page. Active output opens above history;
  completed output remains collapsed behind an explicit Open action.
- Show version, real status, and elapsed/final duration above the output. Use a
  single dark terminal surface capped near 380px with connection state, Follow,
  Resume, and Expand controls. No nested cards or shell input.
- Follow output by default, pause when the user scrolls up, and provide Resume.
  Expand uses the viewport and Escape restores it. Mobile keeps the page fixed;
  only terminal text scrolls horizontally. Reconnect replay must not duplicate
  events, and completed history remains readable without JavaScript.

## Controls and data entry

- Public Source starts with repository URL and Scan repository only. Branch/tag
  and detected applications appear after a successful native scan, using the
  real default branch. URL edits invalidate results; ref changes rescan. Continue
  cannot bypass scanning. Pending, success, empty, and error states are visible.
- Repository and branch controls sit side by side when space permits. Detected
  applications use single-line radio rows for up to three paths and native
  searchable selection for four or more. Each row shows path and build recipe;
  larger repositories keep hidden exact-path form state synchronized with the
  search field. Partial free text is never a valid application selection.
  Selected rows use a restrained green border/tint.
- Environment variables use repeatable rows, never a JSON textarea as the
  primary UI. Desktop proportions: Type 3/12, Name 4/12, Value 4/12, remove
  1/12. Rows preserve exact string values, reject duplicate/invalid names, and
  keep example-file values blank. Zero variables show an empty state, never a
  phantom row. Public scans show a visible pending label only during the real
  request. On mobile each row becomes a stacked group.
- Plain variables and encrypted Secret rows are distinct. Secret values are
  write-only, blank edits preserve them, and they are never rerendered. Never
  offer Secret semantics without encrypted catalog-vault storage.
- Port mapping uses Host port (optional) and Container port controls beside the
  build type. Blank host port displays Automatic; custom values are explicit.
  Helper text states loopback-only binding and automatic allocation. Review and
  summary use an honest `host → container` mapping, with `auto` until assigned.
- Resource defaults use selectable Recommended and Custom cards. Recommended
  shows 1 CPU and 512 MiB in read-only fields. Custom enables editing. Selection
  must be visually obvious without excessive badges.
- Auto-deploy is a single clear checkbox card with accurate behavior copy.
- Only render capabilities the backend supports. Never offer fake Secret,
  static-build, zero-downtime, ready, or success states.

## Responsive and accessible behavior

- No horizontal page overflow at 320px. Stack fields and summary cards before
  labels or values become cramped. Keep touch targets at least about 40px where
  practical and allow long repositories/paths to wrap safely.
- Use semantic forms, labels, fieldsets, buttons, headings, and definition lists.
  Icon-only controls require accessible names. Dynamic errors use an alert or
  associated error text; summaries and toasts must not steal focus.
- Preserve full native POST/CSRF fallback. JavaScript enhances the layout but is
  not the only way to submit. All mutations retain pending, disabled,
  duplicate-submit protection, success, and readable error behavior.

## Implementation discipline

- Reuse Hakobi's SSR components, Tailwind tokens, action controller, and vanilla
  JavaScript conventions. Do not import a second component framework or copy
  demo scripts with timers and simulated success.
- Keep real repository detection, environment discovery, validation, review,
  deployment, and browser history behavior intact.
- Verify the actual production surface at desktop and mobile sizes, including
  keyboard navigation, Back/Continue state, responsive overflow, server errors,
  and one real end-to-end submission.
