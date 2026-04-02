use phf::{phf_map, phf_set};

use crate::ast::SymbolKind;
use SymbolKind::*;

// ---------------------------------------------------------------------------
// LaTeX command → (Unicode character, SymbolKind)
// ---------------------------------------------------------------------------

static LATEX_TO_UNICODE: phf::Map<&'static str, (char, SymbolKind)> = phf_map! {
    // === Uppercase commands ===
    "Delta" => ('\u{0394}', Alphabetic),
    "Downarrow" => ('\u{21D3}', Relation),
    "Gamma" => ('\u{0393}', Alphabetic),
    "Im" => ('\u{2111}', Ordinary),
    "Lambda" => ('\u{039B}', Alphabetic),
    "Leftarrow" => ('\u{21D0}', Relation),
    "Leftrightarrow" => ('\u{21D4}', Relation),
    "Longleftarrow" => ('\u{27F8}', Relation),
    "Longrightarrow" => ('\u{27F9}', Relation),
    "Omega" => ('\u{03A9}', Alphabetic),
    "Phi" => ('\u{03A6}', Alphabetic),
    "Pi" => ('\u{03A0}', Alphabetic),
    "Psi" => ('\u{03A8}', Alphabetic),
    "Re" => ('\u{211C}', Ordinary),
    "Rightarrow" => ('\u{21D2}', Relation),
    "Sigma" => ('\u{03A3}', Alphabetic),
    "Theta" => ('\u{0398}', Alphabetic),
    "Uparrow" => ('\u{21D1}', Relation),
    "Updownarrow" => ('\u{21D5}', Relation),
    "Upsilon" => ('\u{03A5}', Alphabetic),
    "Xi" => ('\u{039E}', Alphabetic),

    // === Lowercase commands ===
    "aleph" => ('\u{2135}', Ordinary),
    "alpha" => ('\u{03B1}', Alphabetic),
    "amalg" => ('\u{2A3F}', BinaryOp),
    "angle" => ('\u{2220}', Ordinary),
    "approx" => ('\u{2248}', Relation),
    "ast" => ('\u{2217}', BinaryOp),
    "asymp" => ('\u{224D}', Relation),
    "backprime" => ('\u{2035}', Ordinary),
    "because" => ('\u{2235}', Relation),
    "beta" => ('\u{03B2}', Alphabetic),
    "beth" => ('\u{2136}', Ordinary),
    "bigcap" => ('\u{22C2}', LargeOp),
    "bigcup" => ('\u{22C3}', LargeOp),
    "bigodot" => ('\u{2A00}', LargeOp),
    "bigoplus" => ('\u{2A01}', LargeOp),
    "bigotimes" => ('\u{2A02}', LargeOp),
    "bigsqcup" => ('\u{2A06}', LargeOp),
    "bigtriangledown" => ('\u{25BD}', BinaryOp),
    "bigtriangleup" => ('\u{25B3}', BinaryOp),
    "biguplus" => ('\u{2A04}', LargeOp),
    "bigvee" => ('\u{22C1}', LargeOp),
    "bigwedge" => ('\u{22C0}', LargeOp),
    "bot" => ('\u{22A5}', Ordinary),
    "bowtie" => ('\u{22C8}', Relation),
    "bullet" => ('\u{2219}', BinaryOp),
    "cap" => ('\u{2229}', BinaryOp),
    "cdot" => ('\u{22C5}', BinaryOp),
    "cdots" => ('\u{22EF}', Ordinary),
    "chi" => ('\u{03C7}', Alphabetic),
    "circ" => ('\u{2218}', BinaryOp),
    "clubsuit" => ('\u{2663}', Ordinary),
    "colon" => (':', Punctuation),
    "complement" => ('\u{2201}', Ordinary),
    "cong" => ('\u{2245}', Relation),
    "coprod" => ('\u{2210}', LargeOp),
    "cup" => ('\u{222A}', BinaryOp),
    "dagger" => ('\u{2020}', BinaryOp),
    "daleth" => ('\u{2138}', Ordinary),
    "dashv" => ('\u{22A3}', Relation),
    "ddagger" => ('\u{2021}', BinaryOp),
    "ddots" => ('\u{22F1}', Ordinary),
    "degree" => ('\u{00B0}', Ordinary),
    "delta" => ('\u{03B4}', Alphabetic),
    "diamond" => ('\u{22C4}', BinaryOp),
    "diamondsuit" => ('\u{2666}', Ordinary),
    "div" => ('\u{00F7}', BinaryOp),
    "doteq" => ('\u{2250}', Relation),
    "dots" => ('\u{2026}', Ordinary),
    "downarrow" => ('\u{2193}', Relation),
    "ell" => ('\u{2113}', Ordinary),
    "emptyset" => ('\u{2205}', Ordinary),
    "epsilon" => ('\u{03F5}', Alphabetic),
    "equiv" => ('\u{2261}', Relation),
    "eta" => ('\u{03B7}', Alphabetic),
    "eth" => ('\u{00F0}', Ordinary),
    "exists" => ('\u{2203}', Ordinary),
    "flat" => ('\u{266D}', Ordinary),
    "forall" => ('\u{2200}', Ordinary),
    "gamma" => ('\u{03B3}', Alphabetic),
    "ge" => ('\u{2265}', Relation),
    "geq" => ('\u{2265}', Relation),
    "geqslant" => ('\u{2A7E}', Relation),
    "gg" => ('\u{226B}', Relation),
    "gimel" => ('\u{2137}', Ordinary),
    "hbar" => ('\u{210F}', Ordinary),
    "heartsuit" => ('\u{2665}', Ordinary),
    "hookleftarrow" => ('\u{21A9}', Relation),
    "hookrightarrow" => ('\u{21AA}', Relation),
    "iff" => ('\u{27FA}', Relation),
    "iiint" => ('\u{222D}', LargeOp),
    "iint" => ('\u{222C}', LargeOp),
    "imath" => ('\u{0131}', Ordinary),
    "implies" => ('\u{27F9}', Relation),
    "in" => ('\u{2208}', Relation),
    "infty" => ('\u{221E}', Ordinary),
    "int" => ('\u{222B}', LargeOp),
    "iota" => ('\u{03B9}', Alphabetic),
    "jmath" => ('\u{0237}', Ordinary),
    "kappa" => ('\u{03BA}', Alphabetic),
    "lVert" => ('\u{2016}', Open),
    "lambda" => ('\u{03BB}', Alphabetic),
    "land" => ('\u{2227}', BinaryOp),
    "langle" => ('\u{27E8}', Open),
    "lbrace" => ('{', Open),
    "lbrack" => ('[', Open),
    "lceil" => ('\u{2308}', Open),
    "ldots" => ('\u{2026}', Ordinary),
    "le" => ('\u{2264}', Relation),
    "leftarrow" => ('\u{2190}', Relation),
    "leftharpoondown" => ('\u{21BD}', Relation),
    "leftharpoonup" => ('\u{21BC}', Relation),
    "leftrightarrow" => ('\u{2194}', Relation),
    "leq" => ('\u{2264}', Relation),
    "leqslant" => ('\u{2A7D}', Relation),
    "lfloor" => ('\u{230A}', Open),
    "ll" => ('\u{226A}', Relation),
    "lnot" => ('\u{00AC}', Ordinary),
    "longleftarrow" => ('\u{27F5}', Relation),
    "longleftrightarrow" => ('\u{27F7}', Relation),
    "longmapsto" => ('\u{27FC}', Relation),
    "longrightarrow" => ('\u{27F6}', Relation),
    "lor" => ('\u{2228}', BinaryOp),
    "lvert" => ('|', Open),
    "mapsto" => ('\u{21A6}', Relation),
    "measuredangle" => ('\u{2221}', Ordinary),
    "mid" => ('\u{2223}', Relation),
    "models" => ('\u{22A8}', Relation),
    "mp" => ('\u{2213}', BinaryOp),
    "mu" => ('\u{03BC}', Alphabetic),
    "nabla" => ('\u{2207}', Ordinary),
    "natural" => ('\u{266E}', Ordinary),
    "ne" => ('\u{2260}', Relation),
    "nearrow" => ('\u{2197}', Relation),
    "neg" => ('\u{00AC}', Ordinary),
    "neq" => ('\u{2260}', Relation),
    "nexists" => ('\u{2204}', Ordinary),
    "ngeq" => ('\u{2271}', Relation),
    "ni" => ('\u{220B}', Relation),
    "nleq" => ('\u{2270}', Relation),
    "nmid" => ('\u{2224}', Relation),
    "notin" => ('\u{2209}', Relation),
    "nprec" => ('\u{2280}', Relation),
    "nsubseteq" => ('\u{2288}', Relation),
    "nsucc" => ('\u{2281}', Relation),
    "nsupseteq" => ('\u{2289}', Relation),
    "nu" => ('\u{03BD}', Alphabetic),
    "nwarrow" => ('\u{2196}', Relation),
    "odot" => ('\u{2299}', BinaryOp),
    "oint" => ('\u{222E}', LargeOp),
    "omega" => ('\u{03C9}', Alphabetic),
    "ominus" => ('\u{2296}', BinaryOp),
    "oplus" => ('\u{2295}', BinaryOp),
    "oslash" => ('\u{2298}', BinaryOp),
    "otimes" => ('\u{2297}', BinaryOp),
    "parallel" => ('\u{2225}', Relation),
    "partial" => ('\u{2202}', Ordinary),
    "perp" => ('\u{22A5}', Relation),
    "phi" => ('\u{03D5}', Alphabetic),
    "pi" => ('\u{03C0}', Alphabetic),
    "pm" => ('\u{00B1}', BinaryOp),
    "prec" => ('\u{227A}', Relation),
    "preceq" => ('\u{2AAF}', Relation),
    "prime" => ('\u{2032}', Ordinary),
    "prod" => ('\u{220F}', LargeOp),
    "propto" => ('\u{221D}', Relation),
    "psi" => ('\u{03C8}', Alphabetic),
    "rVert" => ('\u{2016}', Close),
    "rangle" => ('\u{27E9}', Close),
    "rbrace" => ('}', Close),
    "rbrack" => (']', Close),
    "rceil" => ('\u{2309}', Close),
    "rfloor" => ('\u{230B}', Close),
    "rho" => ('\u{03C1}', Alphabetic),
    "rightarrow" => ('\u{2192}', Relation),
    "rightharpoondown" => ('\u{21C1}', Relation),
    "rightharpoonup" => ('\u{21C0}', Relation),
    "rightleftharpoons" => ('\u{21CC}', Relation),
    "rvert" => ('|', Close),
    "searrow" => ('\u{2198}', Relation),
    "setminus" => ('\u{2216}', BinaryOp),
    "sharp" => ('\u{266F}', Ordinary),
    "sigma" => ('\u{03C3}', Alphabetic),
    "sim" => ('\u{223C}', Relation),
    "simeq" => ('\u{2243}', Relation),
    "spadesuit" => ('\u{2660}', Ordinary),
    "sqcap" => ('\u{2293}', BinaryOp),
    "sqcup" => ('\u{2294}', BinaryOp),
    "sqsubseteq" => ('\u{2291}', Relation),
    "sqsupseteq" => ('\u{2292}', Relation),
    "square" => ('\u{25A1}', Ordinary),
    "star" => ('\u{22C6}', BinaryOp),
    "subset" => ('\u{2282}', Relation),
    "subseteq" => ('\u{2286}', Relation),
    "succ" => ('\u{227B}', Relation),
    "succeq" => ('\u{2AB0}', Relation),
    "sum" => ('\u{2211}', LargeOp),
    "supset" => ('\u{2283}', Relation),
    "supseteq" => ('\u{2287}', Relation),
    "surd" => ('\u{221A}', Ordinary),
    "swarrow" => ('\u{2199}', Relation),
    "tau" => ('\u{03C4}', Alphabetic),
    "therefore" => ('\u{2234}', Relation),
    "theta" => ('\u{03B8}', Alphabetic),
    "times" => ('\u{00D7}', BinaryOp),
    "to" => ('\u{2192}', Relation),
    "top" => ('\u{22A4}', Ordinary),
    "triangle" => ('\u{25B3}', Ordinary),
    "triangleleft" => ('\u{25C1}', BinaryOp),
    "triangleright" => ('\u{25B7}', BinaryOp),
    "uparrow" => ('\u{2191}', Relation),
    "updownarrow" => ('\u{2195}', Relation),
    "upsilon" => ('\u{03C5}', Alphabetic),
    "varepsilon" => ('\u{03B5}', Alphabetic),
    "varkappa" => ('\u{03F0}', Alphabetic),
    "varnothing" => ('\u{2205}', Ordinary),
    "varphi" => ('\u{03C6}', Alphabetic),
    "varpi" => ('\u{03D6}', Alphabetic),
    "varrho" => ('\u{03F1}', Alphabetic),
    "varsigma" => ('\u{03C2}', Alphabetic),
    "vartheta" => ('\u{03D1}', Alphabetic),
    "vdash" => ('\u{22A2}', Relation),
    "vdots" => ('\u{22EE}', Ordinary),
    "vee" => ('\u{2228}', BinaryOp),
    "wedge" => ('\u{2227}', BinaryOp),
    "wp" => ('\u{2118}', Ordinary),
    "wr" => ('\u{2240}', BinaryOp),
    "xi" => ('\u{03BE}', Alphabetic),
    "zeta" => ('\u{03B6}', Alphabetic),
};

// ---------------------------------------------------------------------------
// Unicode character → (preferred LaTeX command, SymbolKind)
// One entry per unique character; for duplicates, the most standard name wins.
// ---------------------------------------------------------------------------

static UNICODE_TO_LATEX: phf::Map<char, (&'static str, SymbolKind)> = phf_map! {
    ':' => ("colon", Punctuation),
    '[' => ("lbrack", Open),
    ']' => ("rbrack", Close),
    '{' => ("lbrace", Open),
    '|' => ("lvert", Open),
    '}' => ("rbrace", Close),
    '\u{00AC}' => ("neg", Ordinary),
    '\u{00B0}' => ("degree", Ordinary),
    '\u{00B1}' => ("pm", BinaryOp),
    '\u{00D7}' => ("times", BinaryOp),
    '\u{00F0}' => ("eth", Ordinary),
    '\u{00F7}' => ("div", BinaryOp),
    '\u{0131}' => ("imath", Ordinary),
    '\u{0237}' => ("jmath", Ordinary),
    '\u{0393}' => ("Gamma", Alphabetic),
    '\u{0394}' => ("Delta", Alphabetic),
    '\u{0398}' => ("Theta", Alphabetic),
    '\u{039B}' => ("Lambda", Alphabetic),
    '\u{039E}' => ("Xi", Alphabetic),
    '\u{03A0}' => ("Pi", Alphabetic),
    '\u{03A3}' => ("Sigma", Alphabetic),
    '\u{03A5}' => ("Upsilon", Alphabetic),
    '\u{03A6}' => ("Phi", Alphabetic),
    '\u{03A8}' => ("Psi", Alphabetic),
    '\u{03A9}' => ("Omega", Alphabetic),
    '\u{03B1}' => ("alpha", Alphabetic),
    '\u{03B2}' => ("beta", Alphabetic),
    '\u{03B3}' => ("gamma", Alphabetic),
    '\u{03B4}' => ("delta", Alphabetic),
    '\u{03B5}' => ("varepsilon", Alphabetic),
    '\u{03B6}' => ("zeta", Alphabetic),
    '\u{03B7}' => ("eta", Alphabetic),
    '\u{03B8}' => ("theta", Alphabetic),
    '\u{03B9}' => ("iota", Alphabetic),
    '\u{03BA}' => ("kappa", Alphabetic),
    '\u{03BB}' => ("lambda", Alphabetic),
    '\u{03BC}' => ("mu", Alphabetic),
    '\u{03BD}' => ("nu", Alphabetic),
    '\u{03BE}' => ("xi", Alphabetic),
    '\u{03C0}' => ("pi", Alphabetic),
    '\u{03C1}' => ("rho", Alphabetic),
    '\u{03C2}' => ("varsigma", Alphabetic),
    '\u{03C3}' => ("sigma", Alphabetic),
    '\u{03C4}' => ("tau", Alphabetic),
    '\u{03C5}' => ("upsilon", Alphabetic),
    '\u{03C6}' => ("varphi", Alphabetic),
    '\u{03C7}' => ("chi", Alphabetic),
    '\u{03C8}' => ("psi", Alphabetic),
    '\u{03C9}' => ("omega", Alphabetic),
    '\u{03D1}' => ("vartheta", Alphabetic),
    '\u{03D5}' => ("phi", Alphabetic),
    '\u{03D6}' => ("varpi", Alphabetic),
    '\u{03F0}' => ("varkappa", Alphabetic),
    '\u{03F1}' => ("varrho", Alphabetic),
    '\u{03F5}' => ("epsilon", Alphabetic),
    '\u{2016}' => ("lVert", Open),
    '\u{2020}' => ("dagger", BinaryOp),
    '\u{2021}' => ("ddagger", BinaryOp),
    '\u{2026}' => ("ldots", Ordinary),
    '\u{2032}' => ("prime", Ordinary),
    '\u{2035}' => ("backprime", Ordinary),
    '\u{210F}' => ("hbar", Ordinary),
    '\u{2111}' => ("Im", Ordinary),
    '\u{2113}' => ("ell", Ordinary),
    '\u{2118}' => ("wp", Ordinary),
    '\u{211C}' => ("Re", Ordinary),
    '\u{2135}' => ("aleph", Ordinary),
    '\u{2136}' => ("beth", Ordinary),
    '\u{2137}' => ("gimel", Ordinary),
    '\u{2138}' => ("daleth", Ordinary),
    '\u{2190}' => ("leftarrow", Relation),
    '\u{2191}' => ("uparrow", Relation),
    '\u{2192}' => ("rightarrow", Relation),
    '\u{2193}' => ("downarrow", Relation),
    '\u{2194}' => ("leftrightarrow", Relation),
    '\u{2195}' => ("updownarrow", Relation),
    '\u{2196}' => ("nwarrow", Relation),
    '\u{2197}' => ("nearrow", Relation),
    '\u{2198}' => ("searrow", Relation),
    '\u{2199}' => ("swarrow", Relation),
    '\u{21A6}' => ("mapsto", Relation),
    '\u{21A9}' => ("hookleftarrow", Relation),
    '\u{21AA}' => ("hookrightarrow", Relation),
    '\u{21BC}' => ("leftharpoonup", Relation),
    '\u{21BD}' => ("leftharpoondown", Relation),
    '\u{21C0}' => ("rightharpoonup", Relation),
    '\u{21C1}' => ("rightharpoondown", Relation),
    '\u{21CC}' => ("rightleftharpoons", Relation),
    '\u{21D0}' => ("Leftarrow", Relation),
    '\u{21D1}' => ("Uparrow", Relation),
    '\u{21D2}' => ("Rightarrow", Relation),
    '\u{21D3}' => ("Downarrow", Relation),
    '\u{21D4}' => ("Leftrightarrow", Relation),
    '\u{21D5}' => ("Updownarrow", Relation),
    '\u{2200}' => ("forall", Ordinary),
    '\u{2201}' => ("complement", Ordinary),
    '\u{2202}' => ("partial", Ordinary),
    '\u{2203}' => ("exists", Ordinary),
    '\u{2204}' => ("nexists", Ordinary),
    '\u{2205}' => ("emptyset", Ordinary),
    '\u{2207}' => ("nabla", Ordinary),
    '\u{2208}' => ("in", Relation),
    '\u{2209}' => ("notin", Relation),
    '\u{220B}' => ("ni", Relation),
    '\u{220F}' => ("prod", LargeOp),
    '\u{2210}' => ("coprod", LargeOp),
    '\u{2211}' => ("sum", LargeOp),
    '\u{2213}' => ("mp", BinaryOp),
    '\u{2216}' => ("setminus", BinaryOp),
    '\u{2217}' => ("ast", BinaryOp),
    '\u{2218}' => ("circ", BinaryOp),
    '\u{2219}' => ("bullet", BinaryOp),
    '\u{221A}' => ("surd", Ordinary),
    '\u{221D}' => ("propto", Relation),
    '\u{221E}' => ("infty", Ordinary),
    '\u{2220}' => ("angle", Ordinary),
    '\u{2221}' => ("measuredangle", Ordinary),
    '\u{2223}' => ("mid", Relation),
    '\u{2224}' => ("nmid", Relation),
    '\u{2225}' => ("parallel", Relation),
    '\u{2227}' => ("wedge", BinaryOp),
    '\u{2228}' => ("vee", BinaryOp),
    '\u{2229}' => ("cap", BinaryOp),
    '\u{222A}' => ("cup", BinaryOp),
    '\u{222B}' => ("int", LargeOp),
    '\u{222C}' => ("iint", LargeOp),
    '\u{222D}' => ("iiint", LargeOp),
    '\u{222E}' => ("oint", LargeOp),
    '\u{2234}' => ("therefore", Relation),
    '\u{2235}' => ("because", Relation),
    '\u{223C}' => ("sim", Relation),
    '\u{2240}' => ("wr", BinaryOp),
    '\u{2243}' => ("simeq", Relation),
    '\u{2245}' => ("cong", Relation),
    '\u{2248}' => ("approx", Relation),
    '\u{224D}' => ("asymp", Relation),
    '\u{2250}' => ("doteq", Relation),
    '\u{2260}' => ("neq", Relation),
    '\u{2261}' => ("equiv", Relation),
    '\u{2264}' => ("leq", Relation),
    '\u{2265}' => ("geq", Relation),
    '\u{226A}' => ("ll", Relation),
    '\u{226B}' => ("gg", Relation),
    '\u{2270}' => ("nleq", Relation),
    '\u{2271}' => ("ngeq", Relation),
    '\u{227A}' => ("prec", Relation),
    '\u{227B}' => ("succ", Relation),
    '\u{2280}' => ("nprec", Relation),
    '\u{2281}' => ("nsucc", Relation),
    '\u{2282}' => ("subset", Relation),
    '\u{2283}' => ("supset", Relation),
    '\u{2286}' => ("subseteq", Relation),
    '\u{2287}' => ("supseteq", Relation),
    '\u{2288}' => ("nsubseteq", Relation),
    '\u{2289}' => ("nsupseteq", Relation),
    '\u{2291}' => ("sqsubseteq", Relation),
    '\u{2292}' => ("sqsupseteq", Relation),
    '\u{2293}' => ("sqcap", BinaryOp),
    '\u{2294}' => ("sqcup", BinaryOp),
    '\u{2295}' => ("oplus", BinaryOp),
    '\u{2296}' => ("ominus", BinaryOp),
    '\u{2297}' => ("otimes", BinaryOp),
    '\u{2298}' => ("oslash", BinaryOp),
    '\u{2299}' => ("odot", BinaryOp),
    '\u{22A2}' => ("vdash", Relation),
    '\u{22A3}' => ("dashv", Relation),
    '\u{22A4}' => ("top", Ordinary),
    '\u{22A5}' => ("bot", Ordinary),
    '\u{22A8}' => ("models", Relation),
    '\u{22C0}' => ("bigwedge", LargeOp),
    '\u{22C1}' => ("bigvee", LargeOp),
    '\u{22C2}' => ("bigcap", LargeOp),
    '\u{22C3}' => ("bigcup", LargeOp),
    '\u{22C4}' => ("diamond", BinaryOp),
    '\u{22C5}' => ("cdot", BinaryOp),
    '\u{22C6}' => ("star", BinaryOp),
    '\u{22C8}' => ("bowtie", Relation),
    '\u{22EE}' => ("vdots", Ordinary),
    '\u{22EF}' => ("cdots", Ordinary),
    '\u{22F1}' => ("ddots", Ordinary),
    '\u{2308}' => ("lceil", Open),
    '\u{2309}' => ("rceil", Close),
    '\u{230A}' => ("lfloor", Open),
    '\u{230B}' => ("rfloor", Close),
    '\u{25A1}' => ("square", Ordinary),
    '\u{25B3}' => ("triangle", Ordinary),
    '\u{25B7}' => ("triangleright", BinaryOp),
    '\u{25BD}' => ("bigtriangledown", BinaryOp),
    '\u{25C1}' => ("triangleleft", BinaryOp),
    '\u{2660}' => ("spadesuit", Ordinary),
    '\u{2663}' => ("clubsuit", Ordinary),
    '\u{2665}' => ("heartsuit", Ordinary),
    '\u{2666}' => ("diamondsuit", Ordinary),
    '\u{266D}' => ("flat", Ordinary),
    '\u{266E}' => ("natural", Ordinary),
    '\u{266F}' => ("sharp", Ordinary),
    '\u{27E8}' => ("langle", Open),
    '\u{27E9}' => ("rangle", Close),
    '\u{27F5}' => ("longleftarrow", Relation),
    '\u{27F6}' => ("longrightarrow", Relation),
    '\u{27F7}' => ("longleftrightarrow", Relation),
    '\u{27F8}' => ("Longleftarrow", Relation),
    '\u{27F9}' => ("Longrightarrow", Relation),
    '\u{27FA}' => ("iff", Relation),
    '\u{27FC}' => ("longmapsto", Relation),
    '\u{2A00}' => ("bigodot", LargeOp),
    '\u{2A01}' => ("bigoplus", LargeOp),
    '\u{2A02}' => ("bigotimes", LargeOp),
    '\u{2A04}' => ("biguplus", LargeOp),
    '\u{2A06}' => ("bigsqcup", LargeOp),
    '\u{2A3F}' => ("amalg", BinaryOp),
    '\u{2A7D}' => ("leqslant", Relation),
    '\u{2A7E}' => ("geqslant", Relation),
    '\u{2AAF}' => ("preceq", Relation),
    '\u{2AB0}' => ("succeq", Relation),
};

// ---------------------------------------------------------------------------
// N-ary operator Unicode → LaTeX command name
// ---------------------------------------------------------------------------

static NARY_OP_LATEX: phf::Map<char, &'static str> = phf_map! {
    '\u{220F}' => "prod",
    '\u{2210}' => "coprod",
    '\u{2211}' => "sum",
    '\u{222B}' => "int",
    '\u{222C}' => "iint",
    '\u{222D}' => "iiint",
    '\u{222E}' => "oint",
    '\u{22C0}' => "bigwedge",
    '\u{22C1}' => "bigvee",
    '\u{22C2}' => "bigcap",
    '\u{22C3}' => "bigcup",
    '\u{2A00}' => "bigodot",
    '\u{2A01}' => "bigoplus",
    '\u{2A02}' => "bigotimes",
    '\u{2A04}' => "biguplus",
    '\u{2A06}' => "bigsqcup",
};

// ---------------------------------------------------------------------------
// Sets for classification checks
// ---------------------------------------------------------------------------

static NARY_COMMANDS: phf::Set<&'static str> = phf_set! {
    "sum", "prod", "coprod", "int", "iint", "iiint", "oint",
    "bigcap", "bigcup", "bigwedge", "bigvee",
    "bigodot", "bigoplus", "bigotimes", "biguplus", "bigsqcup",
};

static OPERATOR_COMMANDS: phf::Set<&'static str> = phf_set! {
    "sin", "cos", "tan", "sec", "csc", "cot",
    "arcsin", "arccos", "arctan",
    "sinh", "cosh", "tanh", "coth",
    "log", "ln", "lg", "exp",
    "lim", "liminf", "limsup",
    "sup", "inf", "min", "max",
    "det", "dim", "ker", "hom", "arg", "deg",
    "gcd", "lcm", "sgn",
    "Pr", "Var", "Cov", "supp", "mod",
};

static LIMIT_OPERATORS: phf::Set<&'static str> = phf_set! {
    "lim", "liminf", "limsup", "sup", "inf", "min", "max", "det", "gcd", "Pr",
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Look up a Unicode character by LaTeX command name (without backslash).
pub fn latex_to_unicode(command: &str) -> Option<char> {
    LATEX_TO_UNICODE.get(command).map(|&(ch, _)| ch)
}

/// Look up a LaTeX command name for a Unicode character.
pub fn unicode_to_latex(ch: char) -> Option<&'static str> {
    UNICODE_TO_LATEX.get(&ch).map(|&(cmd, _)| cmd)
}

/// Look up the math symbol kind for a Unicode character.
pub fn symbol_kind(ch: char) -> SymbolKind {
    UNICODE_TO_LATEX
        .get(&ch)
        .map(|&(_, kind)| kind)
        .unwrap_or(SymbolKind::Ordinary)
}

/// Look up a symbol by LaTeX command name.
/// Returns `(unicode_char, symbol_kind)`.
pub fn lookup_command(command: &str) -> Option<(char, SymbolKind)> {
    LATEX_TO_UNICODE.get(command).copied()
}

/// Look up the LaTeX command for an n-ary operator Unicode character.
pub fn nary_op_to_latex(ch: char) -> Option<&'static str> {
    NARY_OP_LATEX.get(&ch).copied()
}

/// Check whether a LaTeX command is an n-ary (large) operator.
pub fn is_nary_command(command: &str) -> bool {
    NARY_COMMANDS.contains(command)
}

/// Check whether a LaTeX command is a named operator (like sin, cos, lim).
pub fn is_operator_command(command: &str) -> bool {
    OPERATOR_COMMANDS.contains(command)
}

/// Whether a named operator uses limit-style placement by default.
pub fn operator_has_limits(name: &str) -> bool {
    LIMIT_OPERATORS.contains(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greek_lookup() {
        assert_eq!(latex_to_unicode("alpha"), Some('\u{03B1}'));
        assert_eq!(latex_to_unicode("Omega"), Some('\u{03A9}'));
        assert_eq!(latex_to_unicode("nonexistent"), None);
    }

    #[test]
    fn reverse_lookup() {
        assert_eq!(unicode_to_latex('\u{03B1}'), Some("alpha"));
        assert_eq!(unicode_to_latex('\u{2211}'), Some("sum"));
    }

    #[test]
    fn symbol_classification() {
        assert_eq!(symbol_kind('\u{2211}'), SymbolKind::LargeOp);
        assert_eq!(symbol_kind('\u{2264}'), SymbolKind::Relation);
        assert_eq!(symbol_kind('\u{00B1}'), SymbolKind::BinaryOp);
        // Unknown character defaults to Ordinary
        assert_eq!(symbol_kind('Q'), SymbolKind::Ordinary);
    }

    #[test]
    fn nary_ops() {
        assert!(is_nary_command("sum"));
        assert!(is_nary_command("int"));
        assert!(is_nary_command("bigodot"));
        assert!(is_nary_command("bigsqcup"));
        assert!(!is_nary_command("sin"));
    }

    #[test]
    fn operator_commands() {
        assert!(is_operator_command("sin"));
        assert!(is_operator_command("lim"));
        assert!(is_operator_command("lcm"));
        assert!(is_operator_command("sgn"));
        assert!(!is_operator_command("alpha"));
    }

    #[test]
    fn new_symbols_accessible() {
        // Arrows
        assert_eq!(latex_to_unicode("Longrightarrow"), Some('\u{27F9}'));
        assert_eq!(latex_to_unicode("longrightarrow"), Some('\u{27F6}'));
        assert_eq!(latex_to_unicode("updownarrow"), Some('\u{2195}'));
        assert_eq!(latex_to_unicode("nearrow"), Some('\u{2197}'));
        assert_eq!(latex_to_unicode("rightharpoonup"), Some('\u{21C0}'));

        // Logic
        assert_eq!(latex_to_unicode("therefore"), Some('\u{2234}'));
        assert_eq!(latex_to_unicode("because"), Some('\u{2235}'));
        assert_eq!(latex_to_unicode("implies"), Some('\u{27F9}'));
        assert_eq!(latex_to_unicode("iff"), Some('\u{27FA}'));
        assert_eq!(latex_to_unicode("land"), Some('\u{2227}'));
        assert_eq!(latex_to_unicode("lor"), Some('\u{2228}'));
        assert_eq!(latex_to_unicode("top"), Some('\u{22A4}'));
        assert_eq!(latex_to_unicode("bot"), Some('\u{22A5}'));

        // Relations
        assert_eq!(latex_to_unicode("leqslant"), Some('\u{2A7D}'));
        assert_eq!(latex_to_unicode("geqslant"), Some('\u{2A7E}'));
        assert_eq!(latex_to_unicode("preceq"), Some('\u{2AAF}'));
        assert_eq!(latex_to_unicode("succeq"), Some('\u{2AB0}'));
        assert_eq!(latex_to_unicode("simeq"), Some('\u{2243}'));
        assert_eq!(latex_to_unicode("mid"), Some('\u{2223}'));
        assert_eq!(latex_to_unicode("nmid"), Some('\u{2224}'));

        // Binary ops
        assert_eq!(latex_to_unicode("odot"), Some('\u{2299}'));
        assert_eq!(latex_to_unicode("oslash"), Some('\u{2298}'));
        assert_eq!(latex_to_unicode("dagger"), Some('\u{2020}'));
        assert_eq!(latex_to_unicode("amalg"), Some('\u{2A3F}'));

        // Misc
        assert_eq!(latex_to_unicode("beth"), Some('\u{2136}'));
        assert_eq!(latex_to_unicode("gimel"), Some('\u{2137}'));
        assert_eq!(latex_to_unicode("daleth"), Some('\u{2138}'));
        assert_eq!(latex_to_unicode("prime"), Some('\u{2032}'));
        assert_eq!(latex_to_unicode("square"), Some('\u{25A1}'));
        assert_eq!(latex_to_unicode("degree"), Some('\u{00B0}'));
    }

    #[test]
    fn lookup_command_returns_tuple() {
        let (ch, kind) = lookup_command("alpha").unwrap();
        assert_eq!(ch, '\u{03B1}');
        assert_eq!(kind, SymbolKind::Alphabetic);

        let (ch, kind) = lookup_command("sum").unwrap();
        assert_eq!(ch, '\u{2211}');
        assert_eq!(kind, SymbolKind::LargeOp);

        assert!(lookup_command("nonexistent").is_none());
    }

    #[test]
    fn duplicate_commands_resolve() {
        // Both aliases should resolve
        assert_eq!(latex_to_unicode("le"), latex_to_unicode("leq"));
        assert_eq!(latex_to_unicode("ge"), latex_to_unicode("geq"));
        assert_eq!(latex_to_unicode("ne"), latex_to_unicode("neq"));
        assert_eq!(latex_to_unicode("lnot"), latex_to_unicode("neg"));
        assert_eq!(latex_to_unicode("dots"), latex_to_unicode("ldots"));
        assert_eq!(latex_to_unicode("to"), latex_to_unicode("rightarrow"));
    }
}
