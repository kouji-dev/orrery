/**
 * Kill switch for every cost/price surface (A4 estimates, budget guards,
 * settings section, ccusage status-bar readout — the rust twin in
 * src-tauri/src/cost/mod.rs gates the ccusage sampling loop).
 *
 * Off until the estimates are calibrated against actuals (A6): the static
 * heuristic chips read as permanent placeholders. Re-enable by flipping this
 * const and its rust twin.
 */
// Typed `boolean` (not the literal) so ngtsc's template narrowing doesn't
// collapse `costEnabled && …` branches to `never` while the switch is off.
export const COST_FEATURES_ENABLED: boolean = false;
