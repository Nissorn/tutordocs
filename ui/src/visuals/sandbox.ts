/**
 * The visual iframe sandbox, per PROPOSAL.md "Sandboxing agent-generated
 * artifacts": exactly allow-scripts. Opaque origin, no same-origin access,
 * no forms, no top navigation, no popups. Never widen this string.
 */
export const VISUAL_SANDBOX_ATTRIBUTE = "allow-scripts";