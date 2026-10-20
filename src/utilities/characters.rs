#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Ascii,
    Cp437,
}
#[derive(Default)]
pub struct Chart {
    pub page: Page,
    pub cells: [u8; 2],
    pub focus: usize,
}
// CP437 high bytes follow Unicode's CP437.TXT mapping. Low bytes use the
// separately labelled IBM PC display-font glyphs, not control-code decoding.
// https://www.unicode.org/Public/MAPPINGS/VENDORS/MICSFT/PC/CP437.TXT
const HIGH:&str="ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";
const LOW: [char; 32] = [
    '\0', '☺', '☻', '♥', '♦', '♣', '♠', '•', '◘', '○', '◙', '♂', '♀', '♪', '♫', '☼', '►', '◄', '↕',
    '‼', '¶', '§', '▬', '↨', '↑', '↓', '→', '←', '∟', '↔', '▲', '▼',
];
pub fn cp437(byte: u8) -> char {
    match byte {
        0..=31 => LOW[byte as usize],
        127 => '⌂',
        128..=255 => HIGH.chars().nth((byte - 128) as usize).unwrap_or('�'),
        _ => char::from(byte),
    }
}
impl Chart {
    pub fn index(&self) -> usize {
        usize::from(self.page == Page::Cp437)
    }
    pub fn selected(&self) -> u8 {
        self.cells[self.index()]
    }
    pub fn limit(&self) -> u8 {
        if self.page == Page::Ascii {
            127
        } else {
            255
        }
    }
    pub fn select(&mut self, byte: u8) {
        let index = self.index();
        self.cells[index] = byte.min(self.limit());
    }
    pub fn mapped(&self, byte: u8) -> char {
        if self.page == Page::Ascii {
            char::from(byte)
        } else {
            cp437(byte)
        }
    }
    pub fn copy_text(&self) -> Option<String> {
        let c = self.mapped(self.selected());
        if c.is_control() {
            None
        } else {
            Some(c.to_string())
        }
    }
    pub fn details(&self) -> String {
        let b = self.selected();
        let c = self.mapped(b);
        let mut bytes = [0; 4];
        let utf8 = c
            .encode_utf8(&mut bytes)
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "Byte {b} / 0x{b:02X}    U+{:04X}    UTF-8: {utf8}",
            c as u32
        )
    }
}
