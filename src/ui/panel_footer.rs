//! Legacy text adapter; footer values are toolkit-neutral.
use crate::presentation::text::{Line, Span};
use crate::{config::Config, panel::Panel};
fn line(text: crate::panel_status::Line, config: &Config) -> Line<'static> {
    Line::from(
        text.spans
            .into_iter()
            .map(|s| Span::styled(s.content, config.palette.dos(s.attribute)))
            .collect::<Vec<_>>(),
    )
}
pub(super) fn selection(panel: &Panel, config: &Config, width: u16) -> Line<'static> {
    line(crate::panel_status::selection(panel, config, width), config)
}
pub(super) fn space(panel: &Panel, config: &Config, width: u16) -> Line<'static> {
    line(crate::panel_status::space(panel, config, width), config)
}
