pub struct Syntax {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub single_line_comment: &'static str,
    pub multi_line_comment: (&'static str, &'static str),
    pub keywords: &'static [&'static str],
    pub types: &'static [&'static str],
    pub builtins: &'static [&'static str],
    pub string_delimiter: &'static str,
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
        builtins: &[],
        string_delimiter: "\"",
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
        builtins: &[],
        string_delimiter: "\"",
        flags: HIGHLIGHT_NUMBERS | HIGHLIGHT_STRINGS | HIGHLIGHT_CHARS,
    },
    Syntax {
        name: "haskell",
        extensions: &[".hs"],
        single_line_comment: "--",
        multi_line_comment: ("{-", "-}"),
        keywords: &[
            "as",
            "case",
            "of",
            "class",
            "data",
            "data family",
            "data instance",
            "default",
            "deriving",
            "deriving instance",
            "do",
            "forall",
            "foreign",
            "hiding",
            "if",
            "then",
            "else",
            "import",
            "infix",
            "infixl",
            "infixr",
            "instance",
            "let",
            "in",
            "mdo",
            "module",
            "newtype",
            "proc",
            "qualified",
            "rec",
            "type",
            "type family",
            "type instance",
            "where",
        ],
        types: &[
            "Bool",
            "Bounded",
            "Char",
            "Double",
            "Either",
            "Enum",
            "Eq",
            "Float",
            "Floating",
            "Fractional",
            "Functor",
            "IO",
            "Int",
            "Integer",
            "Integral",
            "Maybe",
            "Monad",
            "Num",
            "Ord",
            "Ordering",
            "Rational",
            "Real",
            "RealFloat",
            "RealFrac",
            "String",
        ],
        builtins: &[],
        string_delimiter: "\"",
        flags: HIGHLIGHT_NUMBERS | HIGHLIGHT_STRINGS | HIGHLIGHT_CHARS,
    },
    Syntax {
        name: "python",
        extensions: &[".py"],
        single_line_comment: "#",
        multi_line_comment: ("", ""),
        keywords: &[
            "False", "None", "True", "and", "as", "assert", "async", "await",
            "break", "class", "continue", "def", "del", "elif", "else",
            "except", "finally", "for", "from", "global", "if", "import", "in",
            "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return",
            "try", "while", "with", "yield",
        ],
        types: &["int", "float", "bool", "str", "bytes", "object"],
        builtins: &[
            "abs",
            "all",
            "any",
            "ascii",
            "bin",
            "bool",
            "breakpoint",
            "bytearray",
            "bytes",
            "callable",
            "chr",
            "classmethod",
            "compile",
            "complex",
            "delattr",
            "dict",
            "dir",
            "divmod",
            "enumerate",
            "eval",
            "exec",
            "filter",
            "float",
            "format",
            "frozenset",
            "getattr",
            "globals",
            "hasattr",
            "hash",
            "help",
            "hex",
            "id",
            "input",
            "int",
            "isinstance",
            "issubclass",
            "iter",
            "len",
            "list",
            "locals",
            "map",
            "max",
            "memoryview",
            "min",
            "next",
            "object",
            "oct",
            "open",
            "ord",
            "pow",
            "print",
            "property",
            "range",
            "repr",
            "reversed",
            "round",
            "set",
            "setattr",
            "slice",
            "sorted",
            "staticmethod",
            "str",
            "sum",
            "super",
            "tuple",
            "type",
            "vars",
            "zip",
        ],
        string_delimiter: "\"'",
        flags: HIGHLIGHT_NUMBERS | HIGHLIGHT_STRINGS,
    },
    Syntax {
        name: "shell",
        extensions: &[".sh"],
        single_line_comment: "#",
        multi_line_comment: ("", ""),
        keywords: &[
            "if", "fi", "then", "elif", "else", "return", "let", "local",
            "function", "for", "case", "esac", "while", "do", "done", "in",
            "break", "select", "until",
        ],
        types: &[],
        builtins: &[
            "alias",
            "bg",
            "bind",
            "builtin",
            "caller",
            "cd",
            "command",
            "compgen",
            "complete",
            "compopt",
            "coproc",
            "declare",
            "dirs",
            "disown",
            "echo",
            "enable",
            "eval",
            "exec",
            "export",
            "false",
            "fc",
            "fg",
            "getopts",
            "hash",
            "help",
            "history",
            "jobs",
            "kill",
            "logout",
            "mapfile",
            "popd",
            "printf",
            "pushd",
            "pwd",
            "read",
            "readarray",
            "readonly",
            "set",
            "shift",
            "shopt",
            "source",
            "suspend",
            "test",
            "time",
            "times",
            "trap",
            "true",
            "type",
            "typeset",
            "ulimit",
            "umask",
            "unalias",
            "unset",
            "wait",
        ],
        string_delimiter: "\"'",
        flags: HIGHLIGHT_NUMBERS | HIGHLIGHT_STRINGS | HIGHLIGHT_CHARS,
    },
];
