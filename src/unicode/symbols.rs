use phf::{phf_map, phf_set};

use crate::ast::{LimitStyle, SymbolKind};
use SymbolKind::*;

// ---------------------------------------------------------------------------
// LaTeX command → (Unicode character, SymbolKind)
// ---------------------------------------------------------------------------

static LATEX_TO_UNICODE: phf::Map<&'static str, (char, SymbolKind)> = phf_map! {
    // === Uppercase commands ===
    "Delta" => ('\u{0394}', Alphabetic),
    "Downarrow" => ('\u{21D3}', Relation),
    "Gamma" => ('\u{0393}', Alphabetic),
    "Im" => ('\u{2111}', Alphabetic),
    "Lambda" => ('\u{039B}', Alphabetic),
    "Leftarrow" => ('\u{21D0}', Relation),
    "Leftrightarrow" => ('\u{21D4}', Relation),
    "Longleftarrow" => ('\u{27F8}', Relation),
    "Longleftrightarrow" => ('\u{27FA}', Relation),
    "Longrightarrow" => ('\u{27F9}', Relation),
    "Omega" => ('\u{03A9}', Alphabetic),
    "Phi" => ('\u{03A6}', Alphabetic),
    "Pi" => ('\u{03A0}', Alphabetic),
    "Psi" => ('\u{03A8}', Alphabetic),
    "Re" => ('\u{211C}', Alphabetic),
    "Rightarrow" => ('\u{21D2}', Relation),
    "Sigma" => ('\u{03A3}', Alphabetic),
    "Theta" => ('\u{0398}', Alphabetic),
    "Uparrow" => ('\u{21D1}', Relation),
    "Updownarrow" => ('\u{21D5}', Relation),
    "Upsilon" => ('\u{03A5}', Alphabetic),
    "Xi" => ('\u{039E}', Alphabetic),

    // === Lowercase commands ===
    "aleph" => ('\u{2135}', Alphabetic),
    "alpha" => ('\u{03B1}', Alphabetic),
    "amalg" => ('\u{2A3F}', BinaryOp),
    "angle" => ('\u{2220}', Ordinary),
    "approx" => ('\u{2248}', Relation),
    "ast" => ('\u{2217}', BinaryOp),
    "asymp" => ('\u{224D}', Relation),
    "backprime" => ('\u{2035}', Ordinary),
    "because" => ('\u{2235}', Relation),
    "beta" => ('\u{03B2}', Alphabetic),
    "beth" => ('\u{2136}', Alphabetic),
    "bigcap" => ('\u{22C2}', LargeOp),
    "bigcup" => ('\u{22C3}', LargeOp),
    "bigodot" => ('\u{2A00}', LargeOp),
    "bigoplus" => ('\u{2A01}', LargeOp),
    "bigotimes" => ('\u{2A02}', LargeOp),
    "bigsqcup" => ('\u{2A06}', LargeOp),
    "bigtriangledown" => ('\u{25BD}', BinaryOp),
    "bigtriangleup" => ('\u{25B3}', BinaryOp),
    "biguplus" => ('\u{2A04}', LargeOp),
    "blacklozenge" => ('\u{29EB}', Ordinary),
    "blacksquare" => ('\u{25AA}', Ordinary),
    "blacktriangle" => ('\u{25B2}', Ordinary),
    "blacktriangledown" => ('\u{25BC}', Ordinary),
    "bigvee" => ('\u{22C1}', LargeOp),
    "bigwedge" => ('\u{22C0}', LargeOp),
    "bot" => ('\u{22A5}', Ordinary),
    "bowtie" => ('\u{22C8}', Relation),
    "boxdot" => ('\u{22A1}', BinaryOp),
    "boxminus" => ('\u{229F}', BinaryOp),
    "boxplus" => ('\u{229E}', BinaryOp),
    "boxtimes" => ('\u{22A0}', BinaryOp),
    "bullet" => ('\u{2219}', BinaryOp),
    "cap" => ('\u{2229}', BinaryOp),
    "cdot" => ('\u{22C5}', BinaryOp),
    "cdots" => ('\u{22EF}', Ordinary),
    "centerdot" => ('\u{00B7}', BinaryOp),
    "chi" => ('\u{03C7}', Alphabetic),
    "circ" => ('\u{2218}', BinaryOp),
    "circeq" => ('\u{2257}', Relation),
    "circlearrowleft" => ('\u{21BA}', Relation),
    "circlearrowright" => ('\u{21BB}', Relation),
    "circledast" => ('\u{229B}', BinaryOp),
    "circledcirc" => ('\u{229A}', BinaryOp),
    "circleddash" => ('\u{229D}', BinaryOp),
    "clubsuit" => ('\u{2663}', Ordinary),
    "colon" => (':', Punctuation),
    "complement" => ('\u{2201}', Ordinary),
    "cong" => ('\u{2245}', Relation),
    "coprod" => ('\u{2210}', LargeOp),
    "cup" => ('\u{222A}', BinaryOp),
    "curlyeqprec" => ('\u{22DE}', Relation),
    "curlyeqsucc" => ('\u{22DF}', Relation),
    "curlyvee" => ('\u{22CE}', BinaryOp),
    "curlywedge" => ('\u{22CF}', BinaryOp),
    "curvearrowleft" => ('\u{21B6}', Relation),
    "curvearrowright" => ('\u{21B7}', Relation),
    "dag" => ('\u{2020}', Ordinary),
    "dagger" => ('\u{2020}', BinaryOp),
    "daleth" => ('\u{2138}', Alphabetic),
    "dashv" => ('\u{22A3}', Relation),
    "ddag" => ('\u{2021}', Ordinary),
    "ddagger" => ('\u{2021}', BinaryOp),
    "ddots" => ('\u{22F1}', Ordinary),
    "degree" => ('\u{00B0}', Ordinary),
    "delta" => ('\u{03B4}', Alphabetic),
    "diamond" => ('\u{22C4}', BinaryOp),
    "diamondsuit" => ('\u{2666}', Ordinary),
    "digamma" => ('\u{03DD}', Alphabetic),
    "div" => ('\u{00F7}', BinaryOp),
    "divideontimes" => ('\u{22C7}', BinaryOp),
    "doteq" => ('\u{2250}', Relation),
    "doteqdot" => ('\u{2251}', Relation),
    "dotplus" => ('\u{2214}', BinaryOp),
    "dots" => ('\u{2026}', Ordinary),
    "doublebarwedge" => ('\u{2A5E}', BinaryOp),
    "downarrow" => ('\u{2193}', Relation),
    "ell" => ('\u{2113}', Alphabetic),
    "emptyset" => ('\u{2205}', Ordinary),
    "epsilon" => ('\u{03F5}', Alphabetic),
    "eqcirc" => ('\u{2256}', Relation),
    "eqsim" => ('\u{2242}', Relation),
    "equiv" => ('\u{2261}', Relation),
    "eta" => ('\u{03B7}', Alphabetic),
    "eth" => ('\u{00F0}', Alphabetic),
    "exists" => ('\u{2203}', Ordinary),
    "fallingdotseq" => ('\u{2252}', Relation),
    "flat" => ('\u{266D}', Ordinary),
    "forall" => ('\u{2200}', Ordinary),
    "gamma" => ('\u{03B3}', Alphabetic),
    "ge" => ('\u{2265}', Relation),
    "geq" => ('\u{2265}', Relation),
    "geqq" => ('\u{2267}', Relation),
    "geqslant" => ('\u{2A7E}', Relation),
    "gt" => ('>', Relation),
    "gets" => ('\u{2190}', Relation),
    "gg" => ('\u{226B}', Relation),
    "ggg" => ('\u{22D9}', Relation),
    "gimel" => ('\u{2137}', Alphabetic),
    "gnapprox" => ('\u{2A8A}', Relation),
    "gneq" => ('\u{2A88}', Relation),
    "gnsim" => ('\u{22E7}', Relation),
    "gtrdot" => ('\u{22D7}', Relation),
    "gtreqless" => ('\u{22DB}', Relation),
    "gtrless" => ('\u{2277}', Relation),
    "gtrsim" => ('\u{2273}', Relation),
    "hbar" => ('\u{210F}', Alphabetic),
    "heartsuit" => ('\u{2665}', Ordinary),
    "hookleftarrow" => ('\u{21A9}', Relation),
    "hookrightarrow" => ('\u{21AA}', Relation),
    "iff" => ('\u{27FA}', Relation),
    "iiiint" => ('\u{2A0C}', LargeOp),
    "iiint" => ('\u{222D}', LargeOp),
    "iint" => ('\u{222C}', LargeOp),
    "imath" => ('\u{0131}', Alphabetic),
    "implies" => ('\u{27F9}', Relation),
    "in" => ('\u{2208}', Relation),
    "infty" => ('\u{221E}', Ordinary),
    "int" => ('\u{222B}', LargeOp),
    "intercal" => ('\u{22BA}', BinaryOp),
    "iota" => ('\u{03B9}', Alphabetic),
    "jmath" => ('\u{0237}', Alphabetic),
    "kappa" => ('\u{03BA}', Alphabetic),
    "Vert" => ('\u{2016}', Ordinary),
    "lVert" => ('\u{2016}', Open),
    "lambda" => ('\u{03BB}', Alphabetic),
    "land" => ('\u{2227}', BinaryOp),
    "lt" => ('<', Relation),
    "langle" => ('\u{27E8}', Open),
    "lbrace" => ('{', Open),
    "lbrack" => ('[', Open),
    "lceil" => ('\u{2308}', Open),
    "ldots" => ('\u{2026}', Ordinary),
    "le" => ('\u{2264}', Relation),
    "leftarrow" => ('\u{2190}', Relation),
    "leftharpoondown" => ('\u{21BD}', Relation),
    "leftharpoonup" => ('\u{21BC}', Relation),
    "leftleftarrows" => ('\u{21C7}', Relation),
    "leftrightarrow" => ('\u{2194}', Relation),
    "leftrightarrows" => ('\u{21C6}', Relation),
    "leftrightharpoons" => ('\u{21CB}', Relation),
    "leftrightsquigarrow" => ('\u{21AD}', Relation),
    "leftthreetimes" => ('\u{22CB}', BinaryOp),
    "leq" => ('\u{2264}', Relation),
    "leqq" => ('\u{2266}', Relation),
    "leqslant" => ('\u{2A7D}', Relation),
    "lessapprox" => ('\u{2A85}', Relation),
    "lessdot" => ('\u{22D6}', Relation),
    "lesseqgtr" => ('\u{22DA}', Relation),
    "lessgtr" => ('\u{2276}', Relation),
    "lesssim" => ('\u{2272}', Relation),
    "lfloor" => ('\u{230A}', Open),
    "ll" => ('\u{226A}', Relation),
    "lll" => ('\u{22D8}', Relation),
    "lnapprox" => ('\u{2A89}', Relation),
    "lneq" => ('\u{2A87}', Relation),
    "lnot" => ('\u{00AC}', Ordinary),
    "lnsim" => ('\u{22E6}', Relation),
    "longleftarrow" => ('\u{27F5}', Relation),
    "longleftrightarrow" => ('\u{27F7}', Relation),
    "longmapsto" => ('\u{27FC}', Relation),
    "longrightarrow" => ('\u{27F6}', Relation),
    "lor" => ('\u{2228}', BinaryOp),
    "ltimes" => ('\u{22C9}', BinaryOp),
    "vert" => ('|', Ordinary),
    "lvert" => ('|', Open),
    "lvertneqq" => ('\u{2268}', Relation),
    "maltese" => ('\u{2720}', Ordinary),
    "mapsfrom" => ('\u{21A4}', Relation),
    "mapsto" => ('\u{21A6}', Relation),
    "measuredangle" => ('\u{2221}', Ordinary),
    "mid" => ('\u{2223}', Relation),
    "minuso" => ('\u{29B5}', Ordinary),
    "models" => ('\u{22A8}', Relation),
    "mp" => ('\u{2213}', BinaryOp),
    "mu" => ('\u{03BC}', Alphabetic),
    "multimap" => ('\u{22B8}', Relation),
    "nabla" => ('\u{2207}', Ordinary),
    "natural" => ('\u{266E}', Ordinary),
    "ne" => ('\u{2260}', Relation),
    "nearrow" => ('\u{2197}', Relation),
    "neg" => ('\u{00AC}', Ordinary),
    "neq" => ('\u{2260}', Relation),
    "nexists" => ('\u{2204}', Ordinary),
    "nequiv" => ('\u{2262}', Relation),
    "ngeq" => ('\u{2271}', Relation),
    "ngeqslant" => ('\u{2271}', Relation),
    "ngtr" => ('\u{226F}', Relation),
    "ni" => ('\u{220B}', Relation),
    "nless" => ('\u{226E}', Relation),
    "nleq" => ('\u{2270}', Relation),
    "nleqslant" => ('\u{2270}', Relation),
    "nmid" => ('\u{2224}', Relation),
    "notin" => ('\u{2209}', Relation),
    "nparallel" => ('\u{2226}', Relation),
    "nprec" => ('\u{2280}', Relation),
    "nsim" => ('\u{2241}', Relation),
    "nsubset" => ('\u{2284}', Relation),
    "nsubseteq" => ('\u{2288}', Relation),
    "nsucc" => ('\u{2281}', Relation),
    "nsupset" => ('\u{2285}', Relation),
    "nsupseteq" => ('\u{2289}', Relation),
    "ntriangleleft" => ('\u{22EA}', Relation),
    "ntrianglelefteq" => ('\u{22EC}', Relation),
    "ntriangleright" => ('\u{22EB}', Relation),
    "ntrianglerighteq" => ('\u{22ED}', Relation),
    "nu" => ('\u{03BD}', Alphabetic),
    "nwarrow" => ('\u{2196}', Relation),
    "odot" => ('\u{2299}', BinaryOp),
    "oiint" => ('\u{222F}', LargeOp),
    "oiiint" => ('\u{2230}', LargeOp),
    "oint" => ('\u{222E}', LargeOp),
    "ointctrclockwise" => ('\u{2233}', LargeOp),
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
    "pitchfork" => ('\u{22D4}', Relation),
    "pm" => ('\u{00B1}', BinaryOp),
    "pounds" => ('\u{00A3}', Ordinary),
    "prec" => ('\u{227A}', Relation),
    "preccurlyeq" => ('\u{227C}', Relation),
    "preceq" => ('\u{2AAF}', Relation),
    "precnapprox" => ('\u{2AB9}', Relation),
    "precnsim" => ('\u{22E8}', Relation),
    "precsim" => ('\u{227E}', Relation),
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
    "rightrightarrows" => ('\u{21C9}', Relation),
    "rightsquigarrow" => ('\u{21DD}', Relation),
    "rightthreetimes" => ('\u{22CC}', BinaryOp),
    "risingdotseq" => ('\u{2253}', Relation),
    "rtimes" => ('\u{22CA}', BinaryOp),
    "rvert" => ('|', Close),
    "searrow" => ('\u{2198}', Relation),
    "setminus" => ('\u{2216}', BinaryOp),
    "sharp" => ('\u{266F}', Ordinary),
    "sigma" => ('\u{03C3}', Alphabetic),
    "sim" => ('\u{223C}', Relation),
    "simeq" => ('\u{2243}', Relation),
    "smallfrown" => ('\u{2322}', Relation),
    "smallsetminus" => ('\u{2216}', BinaryOp),
    "smallsmile" => ('\u{2323}', Relation),
    "smile" => ('\u{2323}', Relation),
    "spadesuit" => ('\u{2660}', Ordinary),
    "sqcap" => ('\u{2293}', BinaryOp),
    "sqcup" => ('\u{2294}', BinaryOp),
    "sqsubseteq" => ('\u{2291}', Relation),
    "sqsupseteq" => ('\u{2292}', Relation),
    "square" => ('\u{25A1}', Ordinary),
    "star" => ('\u{22C6}', BinaryOp),
    "subset" => ('\u{2282}', Relation),
    "subseteq" => ('\u{2286}', Relation),
    "subseteqq" => ('\u{2AC5}', Relation),
    "subsetneq" => ('\u{228A}', Relation),
    // The font lacks the double-bar variant U+2ACB; use the available ⊊ (U+228A),
    // which is the conventional rendering.
    "subsetneqq" => ('\u{228A}', Relation),
    "succ" => ('\u{227B}', Relation),
    "succcurlyeq" => ('\u{227D}', Relation),
    "succeq" => ('\u{2AB0}', Relation),
    "succnapprox" => ('\u{2ABA}', Relation),
    "succnsim" => ('\u{22E9}', Relation),
    "succsim" => ('\u{227F}', Relation),
    "sum" => ('\u{2211}', LargeOp),
    "supset" => ('\u{2283}', Relation),
    "supseteq" => ('\u{2287}', Relation),
    "supseteqq" => ('\u{2AC6}', Relation),
    "supsetneq" => ('\u{228B}', Relation),
    // The font lacks the double-bar variant U+2ACC; use the available ⊋ (U+228B).
    "supsetneqq" => ('\u{228B}', Relation),
    "surd" => ('\u{221A}', Ordinary),
    "swarrow" => ('\u{2199}', Relation),
    "tau" => ('\u{03C4}', Alphabetic),
    "textdollar" => ('$', Ordinary),
    "therefore" => ('\u{2234}', Relation),
    "theta" => ('\u{03B8}', Alphabetic),
    "times" => ('\u{00D7}', BinaryOp),
    "to" => ('\u{2192}', Relation),
    "top" => ('\u{22A4}', Ordinary),
    "triangle" => ('\u{25B3}', Ordinary),
    "triangledown" => ('\u{25BD}', Ordinary),
    "triangleleft" => ('\u{25C1}', BinaryOp),
    "trianglelefteq" => ('\u{22B4}', Relation),
    "triangleq" => ('\u{225C}', Relation),
    "triangleright" => ('\u{25B7}', BinaryOp),
    "trianglerighteq" => ('\u{22B5}', Relation),
    "twoheadleftarrow" => ('\u{219E}', Relation),
    "twoheadrightarrow" => ('\u{21A0}', Relation),
    "uparrow" => ('\u{2191}', Relation),
    "updownarrow" => ('\u{2195}', Relation),
    "upharpoonleft" => ('\u{21BF}', Relation),
    "upharpoonright" => ('\u{21BE}', Relation),
    "uplus" => ('\u{228E}', BinaryOp),
    "upsilon" => ('\u{03C5}', Alphabetic),
    "upuparrows" => ('\u{21C8}', Relation),
    "varepsilon" => ('\u{03B5}', Alphabetic),
    "varkappa" => ('\u{03F0}', Alphabetic),
    "varnothing" => ('\u{2205}', Ordinary),
    "varointclockwise" => ('\u{2232}', LargeOp),
    "varphi" => ('\u{03C6}', Alphabetic),
    "varpi" => ('\u{03D6}', Alphabetic),
    "varpropto" => ('\u{221D}', Relation),
    "varrho" => ('\u{03F1}', Alphabetic),
    "varsigma" => ('\u{03C2}', Alphabetic),
    "vartheta" => ('\u{03D1}', Alphabetic),
    "vartriangle" => ('\u{25B3}', Relation),
    "vartriangleleft" => ('\u{22B2}', Relation),
    "vartriangleright" => ('\u{22B3}', Relation),
    "vdash" => ('\u{22A2}', Relation),
    "vdots" => ('\u{22EE}', Ordinary),
    "vee" => ('\u{2228}', BinaryOp),
    "wedge" => ('\u{2227}', BinaryOp),
    "wp" => ('\u{2118}', Ordinary),
    "wr" => ('\u{2240}', BinaryOp),
    "xi" => ('\u{03BE}', Alphabetic),
    "zeta" => ('\u{03B6}', Alphabetic),

    // === KaTeX / MathJax aliases and extensions ===
    //
    // Commands KaTeX accepts that have no plain-TeX definition. Each alias
    // copies the code point and class of the command it abbreviates, so they
    // only extend the reader; the reverse map keeps the canonical spellings.

    // Upright Greek capitals that share a Latin glyph.
    "Alpha" => ('\u{0391}', Alphabetic),
    "Beta" => ('\u{0392}', Alphabetic),
    "Epsilon" => ('\u{0395}', Alphabetic),
    "Zeta" => ('\u{0396}', Alphabetic),
    "Eta" => ('\u{0397}', Alphabetic),
    "Iota" => ('\u{0399}', Alphabetic),
    "Kappa" => ('\u{039A}', Alphabetic),
    "Mu" => ('\u{039C}', Alphabetic),
    "Nu" => ('\u{039D}', Alphabetic),
    "Omicron" => ('\u{039F}', Alphabetic),
    "Rho" => ('\u{03A1}', Alphabetic),
    "Tau" => ('\u{03A4}', Alphabetic),
    "Chi" => ('\u{03A7}', Alphabetic),
    "omicron" => ('\u{03BF}', Alphabetic),
    "thetasym" => ('\u{03D1}', Alphabetic),
    // amsmath italic capitals: MATHEMATICAL ITALIC CAPITAL letters.
    "varGamma" => ('\u{1D6E4}', Alphabetic),
    "varDelta" => ('\u{1D6E5}', Alphabetic),
    "varTheta" => ('\u{1D6E9}', Alphabetic),
    "varLambda" => ('\u{1D6EC}', Alphabetic),
    "varXi" => ('\u{1D6EF}', Alphabetic),
    "varPi" => ('\u{1D6F1}', Alphabetic),
    "varSigma" => ('\u{1D6F4}', Alphabetic),
    "varUpsilon" => ('\u{1D6F6}', Alphabetic),
    "varPhi" => ('\u{1D6F7}', Alphabetic),
    "varPsi" => ('\u{1D6F9}', Alphabetic),
    "varOmega" => ('\u{1D6FA}', Alphabetic),

    // Letterlike symbols (HTML-entity style names).
    "alef" => ('\u{2135}', Alphabetic),
    "alefsym" => ('\u{2135}', Alphabetic),
    "real" => ('\u{211C}', Alphabetic),
    "image" => ('\u{2111}', Alphabetic),
    "weierp" => ('\u{2118}', Ordinary),
    "exist" => ('\u{2203}', Ordinary),
    "empty" => ('\u{2205}', Ordinary),
    "isin" => ('\u{2208}', Relation),
    "owns" => ('\u{220B}', Relation),
    "notni" => ('\u{220C}', Relation),

    // Ordinary symbols.
    "backslash" => ('\\', Ordinary),
    "Box" => ('\u{25A1}', Ordinary),
    "Diamond" => ('\u{25CA}', Ordinary),
    "copyright" => ('\u{00A9}', Ordinary),
    "mathsterling" => ('\u{00A3}', Ordinary),
    "iddots" => ('\u{22F0}', Ordinary),
    "mathellipsis" => ('\u{2026}', Ordinary),
    // amsmath semantic dots: binary/multiplication/integral contexts use the
    // centred form, commas and "other" contexts the baseline form.
    "dotsb" => ('\u{22EF}', Ordinary),
    "dotsc" => ('\u{2026}', Ordinary),
    "dotsi" => ('\u{22EF}', Ordinary),
    "dotsm" => ('\u{22EF}', Ordinary),
    "dotso" => ('\u{2026}', Ordinary),
    "ldotp" => ('.', Punctuation),
    "cdotp" => ('\u{22C5}', Punctuation),

    // Binary operators.
    "And" => ('&', BinaryOp),
    "Dagger" => ('\u{2021}', BinaryOp),
    "bigcirc" => ('\u{25EF}', BinaryOp),
    "doublecap" => ('\u{22D2}', BinaryOp),
    "doublecup" => ('\u{22D3}', BinaryOp),
    "lhd" => ('\u{22B2}', BinaryOp),
    "rhd" => ('\u{22B3}', BinaryOp),
    "unlhd" => ('\u{22B4}', BinaryOp),
    "unrhd" => ('\u{22B5}', BinaryOp),

    // Arrows (HTML-entity style names and amssymb extras).
    "uarr" => ('\u{2191}', Relation),
    "darr" => ('\u{2193}', Relation),
    "larr" => ('\u{2190}', Relation),
    "rarr" => ('\u{2192}', Relation),
    "harr" => ('\u{2194}', Relation),
    "lrarr" => ('\u{2194}', Relation),
    "Uarr" => ('\u{21D1}', Relation),
    "uArr" => ('\u{21D1}', Relation),
    "Darr" => ('\u{21D3}', Relation),
    "dArr" => ('\u{21D3}', Relation),
    "Larr" => ('\u{21D0}', Relation),
    "lArr" => ('\u{21D0}', Relation),
    "Rarr" => ('\u{21D2}', Relation),
    "rArr" => ('\u{21D2}', Relation),
    "Harr" => ('\u{21D4}', Relation),
    "hArr" => ('\u{21D4}', Relation),
    "Lrarr" => ('\u{21D4}', Relation),
    "lrArr" => ('\u{21D4}', Relation),
    "Lsh" => ('\u{21B0}', Relation),
    "Rsh" => ('\u{21B1}', Relation),
    "leadsto" => ('\u{21DD}', Relation),
    "restriction" => ('\u{21BE}', Relation),
    "downharpoonleft" => ('\u{21C3}', Relation),
    "downharpoonright" => ('\u{21C2}', Relation),
    "impliedby" => ('\u{27F8}', Relation),

    // Relations.
    "frown" => ('\u{2322}', Relation),
    "Join" => ('\u{22C8}', Relation),
    "shortmid" => ('\u{2223}', Relation),
    "shortparallel" => ('\u{2225}', Relation),
    "nshortmid" => ('\u{2224}', Relation),
    "nshortparallel" => ('\u{2226}', Relation),
    "thicksim" => ('\u{223C}', Relation),
    "gggtr" => ('\u{22D9}', Relation),
    "llless" => ('\u{22D8}', Relation),
    "lneqq" => ('\u{2268}', Relation),
    "gneqq" => ('\u{2269}', Relation),
    "gvertneqq" => ('\u{2269}', Relation),
    // The vertical-stroke negations have no separate code points; like
    // KaTeX's fallback, they share the slashed single-bar forms.
    "nleqq" => ('\u{2270}', Relation),
    "ngeqq" => ('\u{2271}', Relation),
    "gtrapprox" => ('\u{2A86}', Relation),
    "lesseqqgtr" => ('\u{2A8B}', Relation),
    "gtreqqless" => ('\u{2A8C}', Relation),
    "eqslantgtr" => ('\u{2A96}', Relation),
    "npreceq" => ('\u{22E0}', Relation),
    "nsucceq" => ('\u{22E1}', Relation),
    "precneqq" => ('\u{2AB5}', Relation),
    "succneqq" => ('\u{2AB6}', Relation),
    "precapprox" => ('\u{2AB7}', Relation),
    "succapprox" => ('\u{2AB8}', Relation),
    "nsubseteqq" => ('\u{2288}', Relation),
    "nsupseteqq" => ('\u{2289}', Relation),
    // Same font fallback as `\subsetneqq`/`\supsetneqq` above.
    "varsubsetneqq" => ('\u{228A}', Relation),
    "varsupsetneq" => ('\u{228B}', Relation),
    "varsupsetneqq" => ('\u{228B}', Relation),
    "nVdash" => ('\u{22AE}', Relation),
    "nVDash" => ('\u{22AF}', Relation),
    "origof" => ('\u{22B6}', Relation),
    "imageof" => ('\u{22B7}', Relation),
    "blacktriangleright" => ('\u{25B6}', Relation),

    // Paired delimiters.
    "lang" => ('\u{27E8}', Open),
    "rang" => ('\u{27E9}', Close),
    "lgroup" => ('\u{27EE}', Open),
    "rgroup" => ('\u{27EF}', Close),
    "lmoustache" => ('\u{23B0}', Open),
    "rmoustache" => ('\u{23B1}', Close),
    "llbracket" => ('\u{27E6}', Open),
    "rrbracket" => ('\u{27E7}', Close),
    "lBrace" => ('\u{2983}', Open),
    "rBrace" => ('\u{2984}', Close),
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
    '\u{00F0}' => ("eth", Alphabetic),
    '\u{00F7}' => ("div", BinaryOp),
    '\u{0131}' => ("imath", Alphabetic),
    '\u{0237}' => ("jmath", Alphabetic),
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
    '\u{210F}' => ("hbar", Alphabetic),
    '\u{2111}' => ("Im", Alphabetic),
    '\u{2113}' => ("ell", Alphabetic),
    '\u{2118}' => ("wp", Ordinary),
    '\u{211C}' => ("Re", Alphabetic),
    '\u{2135}' => ("aleph", Alphabetic),
    '\u{2136}' => ("beth", Alphabetic),
    '\u{2137}' => ("gimel", Alphabetic),
    '\u{2138}' => ("daleth", Alphabetic),
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
    '\u{21A4}' => ("mapsfrom", Relation),
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
    '\u{2A0C}' => ("iiiint", LargeOp),
    '\u{222E}' => ("oint", LargeOp),
    '\u{222F}' => ("oiint", LargeOp),
    '\u{2230}' => ("oiiint", LargeOp),
    '\u{2232}' => ("varointclockwise", LargeOp),
    '\u{2233}' => ("ointctrclockwise", LargeOp),
    '\u{2234}' => ("therefore", Relation),
    '\u{2235}' => ("because", Relation),
    '\u{223C}' => ("sim", Relation),
    '\u{2240}' => ("wr", BinaryOp),
    '\u{2241}' => ("nsim", Relation),
    '\u{2243}' => ("simeq", Relation),
    '\u{2245}' => ("cong", Relation),
    '\u{2248}' => ("approx", Relation),
    '\u{224D}' => ("asymp", Relation),
    '\u{2250}' => ("doteq", Relation),
    '\u{2260}' => ("neq", Relation),
    '\u{2261}' => ("equiv", Relation),
    '\u{2262}' => ("nequiv", Relation),
    '\u{2264}' => ("leq", Relation),
    '\u{2265}' => ("geq", Relation),
    '\u{226A}' => ("ll", Relation),
    '\u{226B}' => ("gg", Relation),
    '\u{226E}' => ("nless", Relation),
    '\u{226F}' => ("ngtr", Relation),
    '\u{2270}' => ("nleq", Relation),
    '\u{2271}' => ("ngeq", Relation),
    '\u{227A}' => ("prec", Relation),
    '\u{227B}' => ("succ", Relation),
    '\u{2280}' => ("nprec", Relation),
    '\u{2281}' => ("nsucc", Relation),
    '\u{2282}' => ("subset", Relation),
    '\u{2283}' => ("supset", Relation),
    '\u{2284}' => ("nsubset", Relation),
    '\u{2285}' => ("nsupset", Relation),
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
    // Reverse spellings for glyphs reachable only through KaTeX aliases or
    // amssymb names, so the writer emits a command KaTeX accepts rather than
    // raw Unicode and keeps the atom class. Upright Greek capitals that share
    // Latin shapes (`\Alpha`...) are deliberately absent: those commands are
    // KaTeX-only, so the raw letters are the portable spelling.
    '\u{1D6E4}' => ("varGamma", Alphabetic),
    '\u{1D6E5}' => ("varDelta", Alphabetic),
    '\u{1D6E9}' => ("varTheta", Alphabetic),
    '\u{1D6EC}' => ("varLambda", Alphabetic),
    '\u{1D6EF}' => ("varXi", Alphabetic),
    '\u{1D6F1}' => ("varPi", Alphabetic),
    '\u{1D6F4}' => ("varSigma", Alphabetic),
    '\u{1D6F6}' => ("varUpsilon", Alphabetic),
    '\u{1D6F7}' => ("varPhi", Alphabetic),
    '\u{1D6F9}' => ("varPsi", Alphabetic),
    '\u{1D6FA}' => ("varOmega", Alphabetic),
    '\u{220C}' => ("notni", Relation),
    '\u{25CA}' => ("lozenge", Ordinary),
    '\u{00A9}' => ("copyright", Ordinary),
    '\u{00A3}' => ("pounds", Ordinary),
    '\u{22F0}' => ("iddots", Ordinary),
    '\u{25B2}' => ("blacktriangle", Ordinary),
    '\u{25BC}' => ("blacktriangledown", Ordinary),
    '\u{25EF}' => ("bigcirc", BinaryOp),
    '\u{22D2}' => ("Cap", BinaryOp),
    '\u{22D3}' => ("Cup", BinaryOp),
    '\u{22B2}' => ("lhd", BinaryOp),
    '\u{22B3}' => ("rhd", BinaryOp),
    '\u{22B4}' => ("unlhd", BinaryOp),
    '\u{22B5}' => ("unrhd", BinaryOp),
    '\u{21B0}' => ("Lsh", Relation),
    '\u{21B1}' => ("Rsh", Relation),
    '\u{21DD}' => ("leadsto", Relation),
    '\u{21BE}' => ("upharpoonright", Relation),
    '\u{21C3}' => ("downharpoonleft", Relation),
    '\u{21C2}' => ("downharpoonright", Relation),
    '\u{2322}' => ("frown", Relation),
    '\u{2226}' => ("nparallel", Relation),
    '\u{22D9}' => ("ggg", Relation),
    '\u{22D8}' => ("lll", Relation),
    '\u{2268}' => ("lneqq", Relation),
    '\u{2269}' => ("gneqq", Relation),
    '\u{2A86}' => ("gtrapprox", Relation),
    '\u{2A8B}' => ("lesseqqgtr", Relation),
    '\u{2A8C}' => ("gtreqqless", Relation),
    '\u{2A96}' => ("eqslantgtr", Relation),
    '\u{22E0}' => ("npreceq", Relation),
    '\u{22E1}' => ("nsucceq", Relation),
    '\u{2AB5}' => ("precneqq", Relation),
    '\u{2AB6}' => ("succneqq", Relation),
    '\u{2AB7}' => ("precapprox", Relation),
    '\u{2AB8}' => ("succapprox", Relation),
    '\u{228A}' => ("subsetneq", Relation),
    '\u{228B}' => ("supsetneq", Relation),
    '\u{22AE}' => ("nVdash", Relation),
    '\u{22AF}' => ("nVDash", Relation),
    '\u{22B6}' => ("origof", Relation),
    '\u{22B7}' => ("imageof", Relation),
    '\u{25B6}' => ("blacktriangleright", Relation),
    '\u{27EE}' => ("lgroup", Open),
    '\u{27EF}' => ("rgroup", Close),
    '\u{23B0}' => ("lmoustache", Open),
    '\u{23B1}' => ("rmoustache", Close),
    '\u{27E6}' => ("llbracket", Open),
    '\u{27E7}' => ("rrbracket", Close),
    '\u{2983}' => ("lBrace", Open),
    '\u{2984}' => ("rBrace", Close),
};

// Named functions have word-valued glyphs and are separate from n-ary symbols.
static OPERATOR_COMMANDS: phf::Set<&'static str> = phf_set! {
    "sin", "cos", "tan", "sec", "csc", "cot",
    "arcsin", "arccos", "arctan",
    "sinh", "cosh", "tanh", "coth",
    "log", "ln", "lg", "exp",
    "lim", "liminf", "limsup", "injlim", "projlim",
    "sup", "inf", "min", "max",
    "det", "dim", "ker", "hom", "arg", "deg",
    "gcd", "lcm", "sgn",
    "Pr", "Var", "Cov", "supp", "mod",
    "argmax", "argmin",
    // Russian/Eastern-European names supported by KaTeX.
    "arctg", "arcctg", "ch", "cosec", "cotg", "ctg", "cth", "sh", "tg", "th",
};

/// Compound limit commands whose typeset names contain a thin space.
const COMPOUND_LIMIT_NAMES: &[(&str, &str)] = &[
    ("limsup", "lim\u{2009}sup"),
    ("liminf", "lim\u{2009}inf"),
    ("injlim", "inj\u{2009}lim"),
    ("projlim", "proj\u{2009}lim"),
];

/// Match a complete UnicodeMath function name without scanning arbitrary
/// identifier tails. A compound name may contain its TeX thin space or the
/// no-break space used in UnicodeMath to join the words of one operand.
pub(crate) fn unicode_math_operator_prefix(input: &str) -> Option<(&str, LimitStyle)> {
    if !input
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
    {
        return None;
    }
    // A name ends before a character that is not alphanumeric, so it is the
    // whole run of ASCII letters at the start; no name is longer than 8.
    let letters = input
        .bytes()
        .take(9)
        .take_while(u8::is_ascii_alphabetic)
        .count();
    let boundary = |rest: &str| rest.chars().next().is_none_or(|ch| !ch.is_alphanumeric());
    let (word, rest) = input.split_at(letters);
    if !(2..=8).contains(&letters) || !boundary(rest) {
        return None;
    }
    if rest.starts_with(['\u{2009}', '\u{00a0}']) {
        for &(_, label) in COMPOUND_LIMIT_NAMES {
            let (left, right) = label.split_once('\u{2009}')?;
            if word == left
                && let Some(rest) = rest
                    .strip_prefix(['\u{2009}', '\u{00a0}'])
                    .and_then(|s| s.strip_prefix(right))
                && boundary(rest)
            {
                return Some((&input[..input.len() - rest.len()], LimitStyle::Default));
            }
        }
    }
    let command = if word == "plim" {
        "plim"
    } else {
        OPERATOR_COMMANDS.get_key(word).copied()?
    };
    let limits = if command == "plim" {
        LimitStyle::AboveBelow
    } else if operator_has_limits(command) {
        LimitStyle::Default
    } else {
        LimitStyle::SubSup
    };
    Some((command, limits))
}

pub(crate) fn compound_limit_name(command: &str) -> Option<&'static str> {
    COMPOUND_LIMIT_NAMES
        .iter()
        .find_map(|&(cmd, name)| (cmd == command).then_some(name))
}

pub(crate) fn compound_limit_command(name: &str) -> Option<&'static str> {
    COMPOUND_LIMIT_NAMES
        .iter()
        .find_map(|&(cmd, text)| (text == name).then_some(cmd))
}

static LIMIT_OPERATORS: phf::Set<&'static str> = phf_set! {
    "lim", "liminf", "limsup", "injlim", "projlim",
    "sup", "inf", "min", "max", "det", "gcd", "Pr", "argmax", "argmin",
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Look up a Unicode character by LaTeX command name (without backslash).
#[inline]
pub fn latex_to_unicode(command: &str) -> Option<char> {
    LATEX_TO_UNICODE.get(command).map(|&(ch, _)| ch)
}

/// Look up a LaTeX command name for a Unicode character.
#[inline]
pub fn unicode_to_latex(ch: char) -> Option<&'static str> {
    UNICODE_TO_LATEX.get(&ch).map(|&(cmd, _)| cmd)
}

/// The canonical LaTeX command for `ch` together with its atom class, from
/// one lookup.
#[inline]
pub(crate) fn unicode_to_latex_entry(ch: char) -> Option<(&'static str, SymbolKind)> {
    UNICODE_TO_LATEX.get(&ch).copied()
}

/// Return the declared atom class when the glyph has a canonical control word.
#[inline]
pub(crate) fn mapped_symbol_kind(ch: char) -> Option<SymbolKind> {
    UNICODE_TO_LATEX.get(&ch).map(|&(_, kind)| kind)
}

/// The atom class a lone non-ASCII operator glyph (MathML `<mo>`, an OMML
/// math-run symbol, a UnicodeMath operator) takes from the symbol table, when
/// that class is an operator class. Ordinary and alphabetic entries return
/// `None` so callers keep their own default for them, as do ASCII glyphs:
/// their class is context-dependent (`:` is a TeX relation but the table
/// records `\colon`), so each reader keeps its own reading of them.
#[inline]
pub(crate) fn operator_glyph_kind(ch: char) -> Option<SymbolKind> {
    if ch.is_ascii() {
        return None;
    }
    mapped_symbol_kind(ch)
        .filter(|kind| !matches!(kind, SymbolKind::Ordinary | SymbolKind::Alphabetic))
}

/// The atom class a glyph has where nothing but the glyph says so (a
/// UnicodeMath operator): the one table the UnicodeMath reader, its operand
/// rules and the UnicodeMath writer consult. `None` for a glyph with no class
/// but ordinary or alphabetic.
///
/// In order: the canonical command's class, the class of any other command or
/// package symbol spelling the glyph, the Unicode math class, the MathML
/// operator dictionary's infix spacing, and a relation for an arrow (UTN #28
/// appendix B).
pub(crate) fn glyph_class(ch: char) -> Option<SymbolKind> {
    use crate::unicode::operators::{OperatorForm, lookup_operator};
    let class = match ch {
        // `\perp` and `\bot` share a glyph; UTN #28 appendix B: relational.
        '\u{22A5}' => SymbolKind::Relation,
        // Symmetric delimiters, whatever `\lvert` and `\rvert` say.
        '|' | '\u{2016}' => SymbolKind::Fence,
        '+' | '-' | '*' | '\u{2212}' | '\u{2022}' => SymbolKind::BinaryOp,
        '=' | '<' | '>' | ':' => SymbolKind::Relation,
        ',' | ';' => SymbolKind::Punctuation,
        _ if ch.is_ascii() => return None,
        _ => match mapped_symbol_kind(ch) {
            Some(kind) => kind,
            None => match other_glyph_classes().get(&ch) {
                Some(&kind) => kind,
                None => match crate::unicode::math_class::math_class(ch) {
                    Some(kind) => kind,
                    None => match lookup_operator(ch, OperatorForm::Infix) {
                        Some(entry) if entry.lspace == 5 => SymbolKind::Relation,
                        Some(entry) if entry.lspace == 4 => SymbolKind::BinaryOp,
                        _ => match ch {
                            '\u{2190}'..='\u{21FF}'
                            | '\u{27F0}'..='\u{27FF}'
                            | '\u{2900}'..='\u{297F}'
                            | '\u{2B00}'..='\u{2B4F}' => SymbolKind::Relation,
                            _ => return None,
                        },
                    },
                },
            },
        },
    };
    (!matches!(class, SymbolKind::Ordinary | SymbolKind::Alphabetic)).then_some(class)
}

/// Classes of the glyphs only non-canonical commands and package symbols
/// spell; an operator class wins over an ordinary one.
fn other_glyph_classes() -> &'static std::collections::HashMap<char, SymbolKind> {
    static MAP: std::sync::OnceLock<std::collections::HashMap<char, SymbolKind>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        let packages = crate::latex::packages::PACKAGE_SYMBOLS
            .iter()
            .flat_map(|table| table.iter())
            .filter_map(|&(_, glyph, kind)| {
                let mut chars = glyph.chars();
                chars
                    .next()
                    .filter(|_| chars.next().is_none())
                    .map(|ch| (ch, kind))
            });
        let mut map = std::collections::HashMap::new();
        for (ch, kind) in LATEX_TO_UNICODE.values().copied().chain(packages) {
            if UNICODE_TO_LATEX.contains_key(&ch) {
                continue;
            }
            let known = map.entry(ch).or_insert(kind);
            let rank = |kind: SymbolKind| match kind {
                SymbolKind::Alphabetic => 0,
                SymbolKind::Ordinary => 1,
                SymbolKind::Relation => 3,
                _ => 2,
            };
            if rank(kind) > rank(*known) {
                *known = kind;
            }
        }
        map
    })
}

/// Every LaTeX command of the core tables and packages with its glyph and
/// class, for exhaustive tests.
#[cfg(test)]
pub(crate) fn all_commands() -> Vec<(String, String, SymbolKind)> {
    let mut all: Vec<_> = LATEX_TO_UNICODE
        .entries()
        .map(|(name, &(ch, kind))| ((*name).to_owned(), ch.to_string(), kind))
        .chain(
            crate::latex::packages::PACKAGE_SYMBOLS
                .iter()
                .flat_map(|table| table.iter())
                .map(|&(name, glyph, kind)| (name.to_owned(), glyph.to_owned(), kind)),
        )
        .collect();
    all.sort_by(|a, b| a.0.cmp(&b.0));
    all.dedup_by(|a, b| a.0 == b.0);
    all
}

/// The character for `op` struck through, where Unicode has one: the table of
/// UTN #28 section 4.1 and the other relations and arrows.
#[rustfmt::skip]
pub(crate) fn precomposed_negation(op: char) -> Option<char> {
    Some(match op {
        '<' => '\u{226E}', // < ≮
        '=' => '\u{2260}', // = ≠
        '>' => '\u{226F}', // > ≯
        '\u{2190}' => '\u{219A}', // ← ↚
        '\u{2192}' => '\u{219B}', // → ↛
        '\u{2194}' => '\u{21AE}', // ↔ ↮
        '\u{21D0}' => '\u{21CD}', // ⇐ ⇍
        '\u{21D2}' => '\u{21CF}', // ⇒ ⇏
        '\u{21D4}' => '\u{21CE}', // ⇔ ⇎
        '\u{2203}' => '\u{2204}', // ∃ ∄
        '\u{2208}' => '\u{2209}', // ∈ ∉
        '\u{220B}' => '\u{220C}', // ∋ ∌
        '\u{2223}' => '\u{2224}', // ∣ ∤
        '\u{2225}' => '\u{2226}', // ∥ ∦
        '\u{223C}' => '\u{2241}', // ∼ ≁
        '\u{2243}' => '\u{2244}', // ≃ ≄
        '\u{2245}' => '\u{2247}', // ≅ ≇
        '\u{2248}' => '\u{2249}', // ≈ ≉
        '\u{224D}' => '\u{226D}', // ≍ ≭
        '\u{2261}' => '\u{2262}', // ≡ ≢
        '\u{2264}' => '\u{2270}', // ≤ ≰
        '\u{2265}' => '\u{2271}', // ≥ ≱
        '\u{2272}' => '\u{2274}', // ≲ ≴
        '\u{2273}' => '\u{2275}', // ≳ ≵
        '\u{2276}' => '\u{2278}', // ≶ ≸
        '\u{2277}' => '\u{2279}', // ≷ ≹
        '\u{227A}' => '\u{2280}', // ≺ ⊀
        '\u{227B}' => '\u{2281}', // ≻ ⊁
        '\u{227C}' => '\u{22E0}', // ≼ ⋠
        '\u{227D}' => '\u{22E1}', // ≽ ⋡
        '\u{2282}' => '\u{2284}', // ⊂ ⊄
        '\u{2283}' => '\u{2285}', // ⊃ ⊅
        '\u{2286}' => '\u{2288}', // ⊆ ⊈
        '\u{2287}' => '\u{2289}', // ⊇ ⊉
        '\u{2291}' => '\u{22E2}', // ⊑ ⋢
        '\u{2292}' => '\u{22E3}', // ⊒ ⋣
        '\u{22A2}' => '\u{22AC}', // ⊢ ⊬
        '\u{22A8}' => '\u{22AD}', // ⊨ ⊭
        '\u{22A9}' => '\u{22AE}', // ⊩ ⊮
        '\u{22AB}' => '\u{22AF}', // ⊫ ⊯
        '\u{22B2}' => '\u{22EA}', // ⊲ ⋪
        '\u{22B3}' => '\u{22EB}', // ⊳ ⋫
        '\u{22B4}' => '\u{22EC}', // ⊴ ⋬
        '\u{22B5}' => '\u{22ED}', // ⊵ ⋭
        _ => return None,
    })
}

/// Whether `ch` is a math symbol: some command or package symbol spells it,
/// or it has a Unicode math class.
pub(crate) fn is_symbol_glyph(ch: char) -> bool {
    UNICODE_TO_LATEX.contains_key(&ch)
        || other_glyph_classes().contains_key(&ch)
        || crate::unicode::math_class::math_class(ch).is_some()
}

/// Find a core command spelling `ch` with exactly the atom class `kind`.
///
/// Several commands share a glyph but differ in class (`\triangle` is
/// ordinary, `\vartriangle` a relation, `\bigtriangleup` a binary operator).
/// The writer uses this when the canonical spelling would change the class.
/// Ties resolve to the lexicographically smallest name, so output is stable.
#[cold]
pub(crate) fn command_for_class(ch: char, kind: SymbolKind) -> Option<&'static str> {
    static MAP: std::sync::OnceLock<std::collections::HashMap<(char, SymbolKind), &'static str>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = std::collections::HashMap::new();
        for (&name, &(glyph, class)) in &LATEX_TO_UNICODE {
            map.entry((glyph, class))
                .and_modify(|best: &mut &'static str| {
                    if name < *best {
                        *best = name;
                    }
                })
                .or_insert(name);
        }
        map
    })
    .get(&(ch, kind))
    .copied()
}

/// Whether `ch` is a delimiter TeX accepts after `\left` or `\big`: an
/// opening, closing or fence glyph, a slash or backslash, a vertical arrow, or
/// one of the extensible pieces (`\arrowvert`, `\bracevert`) and corners.
pub(crate) fn is_tex_delimiter(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')'
            | '/'
            | '\\'
            | '|'
            | '\u{2016}'
            | '\u{2191}'
            | '\u{2193}'
            | '\u{2195}'
            | '\u{21D1}'
            | '\u{21D3}'
            | '\u{21D5}'
            | '\u{231C}'..='\u{231F}' | '\u{23AA}' | '\u{23D0}'
    ) || matches!(
        mapped_symbol_kind(ch),
        Some(SymbolKind::Open | SymbolKind::Close | SymbolKind::Fence)
    )
}

/// Look up the math symbol kind for a Unicode character.
#[inline]
pub fn symbol_kind(ch: char) -> SymbolKind {
    mapped_symbol_kind(ch).unwrap_or(SymbolKind::Ordinary)
}

/// Look up a symbol by LaTeX command name.
/// Returns `(unicode_char, symbol_kind)`.
#[inline]
pub fn lookup_command(command: &str) -> Option<(char, SymbolKind)> {
    LATEX_TO_UNICODE.get(command).copied()
}

/// Look up the LaTeX command for an n-ary operator Unicode character.
#[inline]
pub fn nary_op_to_latex(ch: char) -> Option<&'static str> {
    let &(name, kind) = UNICODE_TO_LATEX.get(&ch)?;
    (kind == SymbolKind::LargeOp).then_some(name)
}

/// Identify fixed line and brace decorations whose presentation markup
/// otherwise looks identical to a general accent.
pub(crate) fn fixed_script_decoration(
    glyph: &str,
    position: crate::ast::AccentPosition,
) -> Option<&'static str> {
    use crate::ast::AccentPosition::{Above, Below};
    match (glyph, position) {
        ("‾", Above) => Some("overline"),
        ("‾", Below) => Some("underline"),
        ("⏞", Above) => Some("overbrace"),
        ("⏟", Below) => Some("underbrace"),
        _ => None,
    }
}

/// Glyphs used as a single over- or under-accent in mathematical notation.
#[inline]
pub(crate) fn is_accent_glyph(ch: char) -> bool {
    matches!(
        ch,
        '\u{0300}'..='\u{036F}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{00AF}'
            | '\u{02C9}'
            | '\u{203E}'
            | '\u{23B4}'..='\u{23B5}'
            | '\u{23DC}'..='\u{23DF}'
            | '_'
            | '^'
    )
}

/// Integral operators use side scripts by default in every TeX math style.
#[inline]
pub(crate) fn is_integral_operator(ch: char) -> bool {
    matches!(ch, '∫'..='∰' | '∲' | '∳' | '⨌')
}

/// Check whether a LaTeX command is an n-ary (large) operator.
#[inline]
pub fn is_nary_command(command: &str) -> bool {
    matches!(lookup_command(command), Some((_, SymbolKind::LargeOp)))
}

/// Check whether a LaTeX command is a named operator (like sin, cos, lim).
#[inline]
pub fn is_operator_command(command: &str) -> bool {
    OPERATOR_COMMANDS.contains(command)
}

/// Whether a named operator uses limit-style placement by default.
#[inline]
pub fn operator_has_limits(name: &str) -> bool {
    LIMIT_OPERATORS.contains(name) || compound_limit_command(name).is_some()
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

    #[test]
    fn compound_limit_aliases_are_named_limit_operators() {
        for name in ["limsup", "liminf", "injlim", "projlim"] {
            assert!(is_operator_command(name), "{name}");
            assert!(operator_has_limits(name), "{name}");
        }
        for name in [
            "lim\u{2009}sup",
            "lim\u{2009}inf",
            "inj\u{2009}lim",
            "proj\u{2009}lim",
        ] {
            assert!(operator_has_limits(name), "{name}");
        }
    }
}
