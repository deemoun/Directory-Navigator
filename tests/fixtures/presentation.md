# Presentation regression fixtures

`layout.txt` and `wrapping.txt` were captured before removing the previous UI
package. They contain data only and require no legacy renderer to run.

The layout test covers widths 0–160, nine layouts and spacing 0/1 (2,898 cases).
Feasible allocations must match the frozen geometry. Over-constrained cases
check the owned allocator's bounded degradation policy; application content is
suppressed below 80×25. The wrapping test checks eight text samples at five
widths, including full Unicode graphemes and non-breaking/zero-width spaces.

Do not regenerate these fixtures just to accept a mismatch. Investigate any
visible geometry or text change first. Additional style, width, minimum-allocation
and overflow behavior is checked by independent owned-presentation unit tests.
