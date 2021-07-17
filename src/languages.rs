pub struct Syntax {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub single_line_comment: &'static str,
    pub multi_line_comment: (&'static str, &'static str),
    pub keywords: &'static [&'static str],
    pub types: &'static [&'static str],
    pub flags: u32,
}

pub const HIGHLIGHT_NUMBERS: u32 = 1 << 0;
pub const HIGHLIGHT_STRINGS: u32 = 1 << 1;
pub const HIGHLIGHT_CHARS: u32 = 1 << 2;

pub const SYNTAXES: &[Syntax] = &[
    Syntax {
        name: "c",
        extensions: &[".c", ".h", ".cpp"],
        single_line_comment: "//",
        multi_line_comment: ("/*", "*/"),
        keywords: &[
            "switch", "if", "while", "for", "break", "continue", "return",
            "else", "struct", "union", "typedef", "static", "enum", "class",
            "case",
        ],
        types: &[
            "int", "long", "double", "float", "char", "unsigned", "signed",
            "void",
        ],
        flags: HIGHLIGHT_NUMBERS | HIGHLIGHT_STRINGS | HIGHLIGHT_CHARS,
    },
    Syntax {
        name: "rust",
        extensions: &[".rs"],
        single_line_comment: "//",
        multi_line_comment: ("/*", "*/"),
        keywords: &[
            "as", "break", "const", "continue", "crate", "else", "enum",
            "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
            "match", "mod", "move", "mut", "pub", "ref", "return", "self",
            "Self", "static", "struct", "super", "trait", "true", "type",
            "unsafe", "use", "where", "while", "async", "await", "dyn",
        ],
        types: &[
            "bool", "char", "f32", "f64", "i128", "i16", "i32", "i64", "i8",
            "isize", "str", "u128", "u16", "u32", "u64", "u8", "usize",
        ],
        flags: HIGHLIGHT_NUMBERS | HIGHLIGHT_STRINGS | HIGHLIGHT_CHARS,
    },
];
