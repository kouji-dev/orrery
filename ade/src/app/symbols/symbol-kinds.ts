/**
 * Grammar kind → app-icon name (design orrery-v2 `SYM_KIND_ICON`, plus the
 * kinds the tags.scm captures emit that the mockup did not list). Unknown
 * kinds fall back to the spark glyph, exactly as the design does.
 */
export const SYM_KIND_ICON: Record<string, string> = {
  class: "box",
  struct: "braces",
  interface: "layers",
  trait: "layers",
  fn: "bolt",
  func: "bolt",
  function: "bolt",
  def: "bolt",
  method: "method",
  constructor: "method",
  field: "field",
  property: "field",
  variable: "field",
  enum: "enum",
  variant: "enum",
  type: "tag",
  alias: "tag",
  module: "package",
  namespace: "package",
  package: "package",
  const: "commit",
  constant: "commit",
  static: "commit",
};

export function symbolKindIcon(kind: string | null | undefined): string {
  return SYM_KIND_ICON[(kind ?? "").toLowerCase()] ?? "spark";
}
