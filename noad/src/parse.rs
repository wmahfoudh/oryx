//! The parser: tokens to a math list.
//!
//! Recursive descent with no error path that aborts. Unknown commands
//! become literal atoms, stray closers and alignment markers degrade to
//! literals, and an unclosed group closes at the end of input. Hostile
//! input costs a fallback, never a panic.

use crate::mlist::{
    Atom, AtomClass, ColAlign, Field, Limits, MathList, MathStyle, Noad, TableGaps,
};

/// The symbol vocabulary: command name to codepoint and class, sorted by
/// name for binary search. Coverage grows by adding rows; the layout
/// tests pin every codepoint to a glyph in the STIX fixture.
pub const VOCABULARY: &[(&str, char, AtomClass)] = &[
    // A reserved character, escaped, shows that character, as in KaTeX.
    ("#", '#', AtomClass::Ord),
    ("$", '$', AtomClass::Ord),
    ("%", '%', AtomClass::Ord),
    ("&", '&', AtomClass::Ord),
    ("Alpha", '\u{0391}', AtomClass::Ord),
    ("And", '\u{0026}', AtomClass::Bin),
    ("Arrowvert", '\u{2016}', AtomClass::Ord),
    ("Bbbk", '\u{1D55C}', AtomClass::Ord),
    ("Beta", '\u{0392}', AtomClass::Ord),
    ("Box", '\u{25A1}', AtomClass::Ord),
    ("Bumpeq", '\u{224E}', AtomClass::Rel),
    ("C", '\u{2102}', AtomClass::Ord),
    ("Cap", '\u{22D2}', AtomClass::Bin),
    ("Chi", '\u{03A7}', AtomClass::Ord),
    ("Complex", '\u{2102}', AtomClass::Ord),
    ("Cup", '\u{22D3}', AtomClass::Bin),
    ("Dagger", '\u{2021}', AtomClass::Bin),
    ("Darr", '\u{21D3}', AtomClass::Rel),
    ("Delta", '\u{0394}', AtomClass::Ord),
    ("Diamond", '\u{25CA}', AtomClass::Ord),
    ("Doteq", '\u{2251}', AtomClass::Rel),
    ("Downarrow", '\u{21D3}', AtomClass::Rel),
    ("Epsilon", '\u{0395}', AtomClass::Ord),
    ("Eta", '\u{0397}', AtomClass::Ord),
    ("Finv", '\u{2132}', AtomClass::Ord),
    ("Game", '\u{2141}', AtomClass::Ord),
    ("Gamma", '\u{0393}', AtomClass::Ord),
    ("H", '\u{210D}', AtomClass::Ord),
    ("Harr", '\u{21D4}', AtomClass::Rel),
    ("Im", '\u{2111}', AtomClass::Ord),
    ("Iota", '\u{0399}', AtomClass::Ord),
    ("Join", '\u{22C8}', AtomClass::Rel),
    ("Kappa", '\u{039A}', AtomClass::Ord),
    ("Lambda", '\u{039B}', AtomClass::Ord),
    ("Larr", '\u{21D0}', AtomClass::Rel),
    ("Leftarrow", '\u{21D0}', AtomClass::Rel),
    ("Leftrightarrow", '\u{21D4}', AtomClass::Rel),
    ("Lleftarrow", '\u{21DA}', AtomClass::Rel),
    ("Longleftarrow", '\u{27F8}', AtomClass::Rel),
    ("Longleftrightarrow", '\u{27FA}', AtomClass::Rel),
    ("Longrightarrow", '\u{27F9}', AtomClass::Rel),
    ("Lrarr", '\u{21D4}', AtomClass::Rel),
    ("Lsh", '\u{21B0}', AtomClass::Rel),
    ("Mu", '\u{039C}', AtomClass::Ord),
    ("N", '\u{2115}', AtomClass::Ord),
    ("Nu", '\u{039D}', AtomClass::Ord),
    ("O", '\u{2205}', AtomClass::Ord),
    ("Omega", '\u{03A9}', AtomClass::Ord),
    ("Omicron", '\u{039F}', AtomClass::Ord),
    ("P", '\u{00B6}', AtomClass::Ord),
    ("Phi", '\u{03A6}', AtomClass::Ord),
    ("Pi", '\u{03A0}', AtomClass::Ord),
    ("Psi", '\u{03A8}', AtomClass::Ord),
    ("Q", '\u{211A}', AtomClass::Ord),
    ("R", '\u{211D}', AtomClass::Ord),
    ("Rarr", '\u{21D2}', AtomClass::Rel),
    ("Re", '\u{211C}', AtomClass::Ord),
    ("Reals", '\u{211D}', AtomClass::Ord),
    ("Rho", '\u{03A1}', AtomClass::Ord),
    ("Rightarrow", '\u{21D2}', AtomClass::Rel),
    ("Rrightarrow", '\u{21DB}', AtomClass::Rel),
    ("Rsh", '\u{21B1}', AtomClass::Rel),
    ("S", '\u{00A7}', AtomClass::Ord),
    ("Sigma", '\u{03A3}', AtomClass::Ord),
    ("Subset", '\u{22D0}', AtomClass::Rel),
    ("Supset", '\u{22D1}', AtomClass::Rel),
    ("Tau", '\u{03A4}', AtomClass::Ord),
    ("Theta", '\u{0398}', AtomClass::Ord),
    ("Uarr", '\u{21D1}', AtomClass::Rel),
    ("Uparrow", '\u{21D1}', AtomClass::Rel),
    ("Updownarrow", '\u{21D5}', AtomClass::Rel),
    ("Upsilon", '\u{03A5}', AtomClass::Ord),
    ("Vdash", '\u{22A9}', AtomClass::Rel),
    ("Vert", '\u{2016}', AtomClass::Ord),
    ("Vvdash", '\u{22AA}', AtomClass::Rel),
    ("Xi", '\u{039E}', AtomClass::Ord),
    ("Z", '\u{2124}', AtomClass::Ord),
    ("Zeta", '\u{0396}', AtomClass::Ord),
    ("_", '_', AtomClass::Ord),
    ("alef", '\u{2135}', AtomClass::Ord),
    ("alefsym", '\u{2135}', AtomClass::Ord),
    ("aleph", '\u{2135}', AtomClass::Ord),
    ("alpha", '\u{03B1}', AtomClass::Ord),
    ("amalg", '\u{2A3F}', AtomClass::Bin),
    ("and", '\u{2227}', AtomClass::Bin),
    ("ang", '\u{2220}', AtomClass::Ord),
    ("angle", '\u{2220}', AtomClass::Ord),
    ("approx", '\u{2248}', AtomClass::Rel),
    ("approxeq", '\u{224A}', AtomClass::Rel),
    ("arrowvert", '|', AtomClass::Ord),
    ("ast", '\u{2217}', AtomClass::Bin),
    ("asymp", '\u{224D}', AtomClass::Rel),
    ("backepsilon", '\u{220D}', AtomClass::Rel),
    ("backprime", '\u{2035}', AtomClass::Ord),
    ("backsim", '\u{223D}', AtomClass::Rel),
    ("backsimeq", '\u{22CD}', AtomClass::Rel),
    ("backslash", '\u{005C}', AtomClass::Ord),
    ("barwedge", '\u{22BC}', AtomClass::Bin),
    ("because", '\u{2235}', AtomClass::Rel),
    ("beta", '\u{03B2}', AtomClass::Ord),
    ("beth", '\u{2136}', AtomClass::Ord),
    ("between", '\u{226C}', AtomClass::Rel),
    ("bigcap", '\u{22C2}', AtomClass::Op),
    ("bigcirc", '\u{25EF}', AtomClass::Bin),
    ("bigcup", '\u{22C3}', AtomClass::Op),
    ("bigodot", '\u{2A00}', AtomClass::Op),
    ("bigoplus", '\u{2A01}', AtomClass::Op),
    ("bigotimes", '\u{2A02}', AtomClass::Op),
    ("bigsqcup", '\u{2A06}', AtomClass::Op),
    ("bigstar", '\u{2605}', AtomClass::Ord),
    ("bigtriangledown", '\u{25BD}', AtomClass::Bin),
    ("bigtriangleup", '\u{25B3}', AtomClass::Bin),
    ("biguplus", '\u{2A04}', AtomClass::Op),
    ("bigvee", '\u{22C1}', AtomClass::Op),
    ("bigwedge", '\u{22C0}', AtomClass::Op),
    ("blacklozenge", '\u{29EB}', AtomClass::Ord),
    ("blacksquare", '\u{25A0}', AtomClass::Ord),
    ("blacktriangle", '\u{25B2}', AtomClass::Ord),
    ("blacktriangledown", '\u{25BC}', AtomClass::Ord),
    ("blacktriangleleft", '\u{25C0}', AtomClass::Bin),
    ("blacktriangleright", '\u{25B6}', AtomClass::Bin),
    ("bot", '\u{22A5}', AtomClass::Ord),
    ("bowtie", '\u{22C8}', AtomClass::Rel),
    ("boxdot", '\u{22A1}', AtomClass::Bin),
    ("boxminus", '\u{229F}', AtomClass::Bin),
    ("boxplus", '\u{229E}', AtomClass::Bin),
    ("boxtimes", '\u{22A0}', AtomClass::Bin),
    ("bracevert", '|', AtomClass::Ord),
    ("bull", '\u{2219}', AtomClass::Bin),
    ("bullet", '\u{2219}', AtomClass::Bin),
    ("bumpeq", '\u{224F}', AtomClass::Rel),
    ("cap", '\u{2229}', AtomClass::Bin),
    ("cdot", '\u{22C5}', AtomClass::Bin),
    ("cdotp", '\u{22C5}', AtomClass::Punct),
    ("cdots", '\u{22EF}', AtomClass::Ord),
    ("centerdot", '\u{22C5}', AtomClass::Bin),
    ("checkmark", '\u{2713}', AtomClass::Ord),
    ("chi", '\u{03C7}', AtomClass::Ord),
    ("circ", '\u{2218}', AtomClass::Bin),
    ("circeq", '\u{2257}', AtomClass::Rel),
    ("circlearrowleft", '\u{21BA}', AtomClass::Rel),
    ("circlearrowright", '\u{21BB}', AtomClass::Rel),
    ("circledS", '\u{24C8}', AtomClass::Ord),
    ("circledast", '\u{229B}', AtomClass::Bin),
    ("circledcirc", '\u{229A}', AtomClass::Bin),
    ("circleddash", '\u{229D}', AtomClass::Bin),
    ("clubs", '\u{2663}', AtomClass::Ord),
    ("clubsuit", '\u{2663}', AtomClass::Ord),
    ("cnums", '\u{2102}', AtomClass::Ord),
    ("colon", '\u{003A}', AtomClass::Punct),
    ("coloneqq", '\u{2254}', AtomClass::Rel),
    ("complement", '\u{2201}', AtomClass::Ord),
    ("cong", '\u{2245}', AtomClass::Rel),
    ("coprod", '\u{2210}', AtomClass::Op),
    ("cup", '\u{222A}', AtomClass::Bin),
    ("curlyeqprec", '\u{22DE}', AtomClass::Rel),
    ("curlyeqsucc", '\u{22DF}', AtomClass::Rel),
    ("curlyvee", '\u{22CE}', AtomClass::Bin),
    ("curlywedge", '\u{22CF}', AtomClass::Bin),
    ("curvearrowleft", '\u{21B6}', AtomClass::Rel),
    ("curvearrowright", '\u{21B7}', AtomClass::Rel),
    ("dArr", '\u{21D3}', AtomClass::Rel),
    ("dag", '\u{2020}', AtomClass::Ord),
    ("dagger", '\u{2020}', AtomClass::Bin),
    ("daleth", '\u{2138}', AtomClass::Ord),
    ("darr", '\u{2193}', AtomClass::Rel),
    ("dashleftarrow", '\u{21E0}', AtomClass::Rel),
    ("dashrightarrow", '\u{21E2}', AtomClass::Rel),
    ("dashv", '\u{22A3}', AtomClass::Rel),
    ("ddag", '\u{2021}', AtomClass::Ord),
    ("ddagger", '\u{2021}', AtomClass::Bin),
    ("ddots", '\u{22F1}', AtomClass::Ord),
    ("degree", '\u{00B0}', AtomClass::Ord),
    ("delta", '\u{03B4}', AtomClass::Ord),
    ("diagdown", '\u{2572}', AtomClass::Ord),
    ("diagup", '\u{2571}', AtomClass::Ord),
    ("diamond", '\u{22C4}', AtomClass::Bin),
    ("diamonds", '\u{2662}', AtomClass::Ord),
    ("diamondsuit", '\u{2662}', AtomClass::Ord),
    ("digamma", '\u{03DD}', AtomClass::Ord),
    ("div", '\u{00F7}', AtomClass::Bin),
    ("divideontimes", '\u{22C7}', AtomClass::Bin),
    ("doteq", '\u{2250}', AtomClass::Rel),
    ("doteqdot", '\u{2251}', AtomClass::Rel),
    ("dotplus", '\u{2214}', AtomClass::Bin),
    ("dots", '\u{2026}', AtomClass::Ord),
    ("dotsb", '\u{22EF}', AtomClass::Ord),
    ("dotsc", '\u{2026}', AtomClass::Ord),
    ("dotsi", '\u{22EF}', AtomClass::Ord),
    ("dotsm", '\u{22EF}', AtomClass::Ord),
    ("doublebarwedge", '\u{2A5E}', AtomClass::Bin),
    ("doublecap", '\u{22D2}', AtomClass::Bin),
    ("doublecup", '\u{22D3}', AtomClass::Bin),
    ("downarrow", '\u{2193}', AtomClass::Rel),
    ("downdownarrows", '\u{21CA}', AtomClass::Rel),
    ("downharpoonleft", '\u{21C3}', AtomClass::Rel),
    ("downharpoonright", '\u{21C2}', AtomClass::Rel),
    ("ell", '\u{2113}', AtomClass::Ord),
    ("empty", '\u{2205}', AtomClass::Ord),
    ("emptyset", '\u{2205}', AtomClass::Ord),
    ("epsilon", '\u{03F5}', AtomClass::Ord),
    ("eqcirc", '\u{2256}', AtomClass::Rel),
    ("eqqcolon", '\u{2255}', AtomClass::Rel),
    ("eqsim", '\u{2242}', AtomClass::Rel),
    ("eqslantgtr", '\u{2A96}', AtomClass::Rel),
    ("eqslantless", '\u{2A95}', AtomClass::Rel),
    ("equiv", '\u{2261}', AtomClass::Rel),
    ("eta", '\u{03B7}', AtomClass::Ord),
    ("eth", '\u{00F0}', AtomClass::Ord),
    ("exist", '\u{2203}', AtomClass::Ord),
    ("exists", '\u{2203}', AtomClass::Ord),
    ("fallingdotseq", '\u{2252}', AtomClass::Rel),
    ("flat", '\u{266D}', AtomClass::Ord),
    ("forall", '\u{2200}', AtomClass::Ord),
    ("frown", '\u{2322}', AtomClass::Rel),
    ("gamma", '\u{03B3}', AtomClass::Ord),
    ("ge", '\u{2265}', AtomClass::Rel),
    ("geq", '\u{2265}', AtomClass::Rel),
    ("geqq", '\u{2267}', AtomClass::Rel),
    ("geqslant", '\u{2A7E}', AtomClass::Rel),
    ("gets", '\u{2190}', AtomClass::Rel),
    ("gg", '\u{226B}', AtomClass::Rel),
    ("ggg", '\u{22D9}', AtomClass::Rel),
    ("gggtr", '\u{22D9}', AtomClass::Rel),
    ("gimel", '\u{2137}', AtomClass::Ord),
    ("gnapprox", '\u{2A8A}', AtomClass::Rel),
    ("gneq", '\u{2A88}', AtomClass::Rel),
    ("gneqq", '\u{2269}', AtomClass::Rel),
    ("gnsim", '\u{22E7}', AtomClass::Rel),
    ("gt", '>', AtomClass::Rel),
    ("gtrapprox", '\u{2A86}', AtomClass::Rel),
    ("gtrdot", '\u{22D7}', AtomClass::Rel),
    ("gtreqless", '\u{22DB}', AtomClass::Rel),
    ("gtreqqless", '\u{2A8C}', AtomClass::Rel),
    ("gtrless", '\u{2277}', AtomClass::Rel),
    ("gtrsim", '\u{2273}', AtomClass::Rel),
    ("hArr", '\u{21D4}', AtomClass::Rel),
    ("harr", '\u{2194}', AtomClass::Rel),
    ("hbar", '\u{210F}', AtomClass::Ord),
    ("hearts", '\u{2661}', AtomClass::Ord),
    ("heartsuit", '\u{2661}', AtomClass::Ord),
    ("hookleftarrow", '\u{21A9}', AtomClass::Rel),
    ("hookrightarrow", '\u{21AA}', AtomClass::Rel),
    ("hslash", '\u{210F}', AtomClass::Ord),
    ("iff", '\u{27FA}', AtomClass::Rel),
    ("iiiint", '\u{2A0C}', AtomClass::Op),
    ("iiint", '\u{222D}', AtomClass::Op),
    ("iint", '\u{222C}', AtomClass::Op),
    ("image", '\u{2111}', AtomClass::Ord),
    ("imath", '\u{1D6A4}', AtomClass::Ord),
    ("impliedby", '\u{27F8}', AtomClass::Rel),
    ("implies", '\u{27F9}', AtomClass::Rel),
    ("in", '\u{2208}', AtomClass::Rel),
    ("infin", '\u{221E}', AtomClass::Ord),
    ("infty", '\u{221E}', AtomClass::Ord),
    ("int", '\u{222B}', AtomClass::Op),
    ("intercal", '\u{22BA}', AtomClass::Bin),
    ("iota", '\u{03B9}', AtomClass::Ord),
    ("isin", '\u{2208}', AtomClass::Rel),
    ("jmath", '\u{1D6A5}', AtomClass::Ord),
    ("kappa", '\u{03BA}', AtomClass::Ord),
    ("lArr", '\u{21D0}', AtomClass::Rel),
    ("lVert", '\u{2016}', AtomClass::Open),
    ("lambda", '\u{03BB}', AtomClass::Ord),
    ("land", '\u{2227}', AtomClass::Bin),
    ("lang", '\u{27E8}', AtomClass::Open),
    ("langle", '\u{27E8}', AtomClass::Open),
    ("larr", '\u{2190}', AtomClass::Rel),
    ("lbrace", '\u{007B}', AtomClass::Open),
    ("lbrack", '\u{005B}', AtomClass::Open),
    ("lceil", '\u{2308}', AtomClass::Open),
    ("ldotp", '.', AtomClass::Punct),
    ("ldots", '\u{2026}', AtomClass::Ord),
    ("le", '\u{2264}', AtomClass::Rel),
    ("leadsto", '\u{21DD}', AtomClass::Rel),
    ("leftarrow", '\u{2190}', AtomClass::Rel),
    ("leftarrowtail", '\u{21A2}', AtomClass::Rel),
    ("leftharpoondown", '\u{21BD}', AtomClass::Rel),
    ("leftharpoonup", '\u{21BC}', AtomClass::Rel),
    ("leftleftarrows", '\u{21C7}', AtomClass::Rel),
    ("leftrightarrow", '\u{2194}', AtomClass::Rel),
    ("leftrightarrows", '\u{21C6}', AtomClass::Rel),
    ("leftrightharpoons", '\u{21CB}', AtomClass::Rel),
    ("leftrightsquigarrow", '\u{21AD}', AtomClass::Rel),
    ("leftthreetimes", '\u{22CB}', AtomClass::Bin),
    ("leq", '\u{2264}', AtomClass::Rel),
    ("leqq", '\u{2266}', AtomClass::Rel),
    ("leqslant", '\u{2A7D}', AtomClass::Rel),
    ("lessapprox", '\u{2A85}', AtomClass::Rel),
    ("lessdot", '\u{22D6}', AtomClass::Rel),
    ("lesseqgtr", '\u{22DA}', AtomClass::Rel),
    ("lesseqqgtr", '\u{2A8B}', AtomClass::Rel),
    ("lessgtr", '\u{2276}', AtomClass::Rel),
    ("lesssim", '\u{2272}', AtomClass::Rel),
    ("lfloor", '\u{230A}', AtomClass::Open),
    ("lgroup", '\u{27EE}', AtomClass::Open),
    ("lhd", '\u{22B2}', AtomClass::Bin),
    ("ll", '\u{226A}', AtomClass::Rel),
    ("llcorner", '\u{231E}', AtomClass::Open),
    ("lll", '\u{22D8}', AtomClass::Rel),
    ("llless", '\u{22D8}', AtomClass::Rel),
    ("lmoustache", '\u{23B0}', AtomClass::Open),
    ("lnapprox", '\u{2A89}', AtomClass::Rel),
    ("lneq", '\u{2A87}', AtomClass::Rel),
    ("lneqq", '\u{2268}', AtomClass::Rel),
    ("lnot", '\u{00AC}', AtomClass::Ord),
    ("lnsim", '\u{22E6}', AtomClass::Rel),
    ("longleftarrow", '\u{27F5}', AtomClass::Rel),
    ("longleftrightarrow", '\u{27F7}', AtomClass::Rel),
    ("longmapsto", '\u{27FC}', AtomClass::Rel),
    ("longrightarrow", '\u{27F6}', AtomClass::Rel),
    ("looparrowleft", '\u{21AB}', AtomClass::Rel),
    ("looparrowright", '\u{21AC}', AtomClass::Rel),
    ("lor", '\u{2228}', AtomClass::Bin),
    ("lozenge", '\u{25CA}', AtomClass::Ord),
    ("lrArr", '\u{21D4}', AtomClass::Rel),
    ("lrarr", '\u{2194}', AtomClass::Rel),
    ("lrcorner", '\u{231F}', AtomClass::Close),
    ("lt", '<', AtomClass::Rel),
    ("ltimes", '\u{22C9}', AtomClass::Bin),
    ("lvert", '\u{007C}', AtomClass::Open),
    ("mapsto", '\u{21A6}', AtomClass::Rel),
    ("measuredangle", '\u{2221}', AtomClass::Ord),
    ("mho", '\u{2127}', AtomClass::Ord),
    ("mid", '\u{2223}', AtomClass::Rel),
    ("models", '\u{22A8}', AtomClass::Rel),
    ("mp", '\u{2213}', AtomClass::Bin),
    ("mu", '\u{03BC}', AtomClass::Ord),
    ("multimap", '\u{22B8}', AtomClass::Rel),
    ("nLeftarrow", '\u{21CD}', AtomClass::Rel),
    ("nLeftrightarrow", '\u{21CE}', AtomClass::Rel),
    ("nRightarrow", '\u{21CF}', AtomClass::Rel),
    ("nVDash", '\u{22AF}', AtomClass::Rel),
    ("nVdash", '\u{22AE}', AtomClass::Rel),
    ("nabla", '\u{2207}', AtomClass::Ord),
    ("natnums", '\u{2115}', AtomClass::Ord),
    ("natural", '\u{266E}', AtomClass::Ord),
    ("ncong", '\u{2247}', AtomClass::Rel),
    ("ne", '\u{2260}', AtomClass::Rel),
    ("nearrow", '\u{2197}', AtomClass::Rel),
    ("neg", '\u{00AC}', AtomClass::Ord),
    ("neq", '\u{2260}', AtomClass::Rel),
    ("nexists", '\u{2204}', AtomClass::Ord),
    ("ngeq", '\u{2271}', AtomClass::Rel),
    ("ngtr", '\u{226F}', AtomClass::Rel),
    ("ni", '\u{220B}', AtomClass::Rel),
    ("nleftarrow", '\u{219A}', AtomClass::Rel),
    ("nleftrightarrow", '\u{21AE}', AtomClass::Rel),
    ("nleq", '\u{2270}', AtomClass::Rel),
    ("nless", '\u{226E}', AtomClass::Rel),
    ("nmid", '\u{2224}', AtomClass::Rel),
    ("notin", '\u{2209}', AtomClass::Rel),
    ("notni", '\u{220C}', AtomClass::Rel),
    ("nparallel", '\u{2226}', AtomClass::Rel),
    ("nprec", '\u{2280}', AtomClass::Rel),
    ("npreceq", '\u{22E0}', AtomClass::Rel),
    ("nrightarrow", '\u{219B}', AtomClass::Rel),
    ("nsim", '\u{2241}', AtomClass::Rel),
    ("nsubseteq", '\u{2288}', AtomClass::Rel),
    ("nsucc", '\u{2281}', AtomClass::Rel),
    ("nsucceq", '\u{22E1}', AtomClass::Rel),
    ("nsupseteq", '\u{2289}', AtomClass::Rel),
    ("ntriangleleft", '\u{22EA}', AtomClass::Rel),
    ("ntrianglelefteq", '\u{22EC}', AtomClass::Rel),
    ("ntriangleright", '\u{22EB}', AtomClass::Rel),
    ("ntrianglerighteq", '\u{22ED}', AtomClass::Rel),
    ("nu", '\u{03BD}', AtomClass::Ord),
    ("nvDash", '\u{22AD}', AtomClass::Rel),
    ("nvdash", '\u{22AC}', AtomClass::Rel),
    ("nwarrow", '\u{2196}', AtomClass::Rel),
    ("odot", '\u{2299}', AtomClass::Bin),
    ("oiiint", '\u{2230}', AtomClass::Op),
    ("oiint", '\u{222F}', AtomClass::Op),
    ("oint", '\u{222E}', AtomClass::Op),
    ("omega", '\u{03C9}', AtomClass::Ord),
    ("omicron", '\u{03BF}', AtomClass::Ord),
    ("ominus", '\u{2296}', AtomClass::Bin),
    ("oplus", '\u{2295}', AtomClass::Bin),
    ("or", '\u{2228}', AtomClass::Bin),
    ("oslash", '\u{2298}', AtomClass::Bin),
    ("otimes", '\u{2297}', AtomClass::Bin),
    ("owns", '\u{220B}', AtomClass::Rel),
    ("parallel", '\u{2225}', AtomClass::Rel),
    ("part", '\u{2202}', AtomClass::Ord),
    ("partial", '\u{2202}', AtomClass::Ord),
    ("perp", '\u{22A5}', AtomClass::Rel),
    ("phi", '\u{03D5}', AtomClass::Ord),
    ("pi", '\u{03C0}', AtomClass::Ord),
    ("pitchfork", '\u{22D4}', AtomClass::Rel),
    ("plusmn", '\u{00B1}', AtomClass::Bin),
    ("pm", '\u{00B1}', AtomClass::Bin),
    ("prec", '\u{227A}', AtomClass::Rel),
    ("precapprox", '\u{2AB7}', AtomClass::Rel),
    ("preccurlyeq", '\u{227C}', AtomClass::Rel),
    ("preceq", '\u{2AAF}', AtomClass::Rel),
    ("precnapprox", '\u{2AB9}', AtomClass::Rel),
    ("precneqq", '\u{2AB5}', AtomClass::Rel),
    ("precnsim", '\u{22E8}', AtomClass::Rel),
    ("precsim", '\u{227E}', AtomClass::Rel),
    ("prime", '\u{2032}', AtomClass::Ord),
    ("prod", '\u{220F}', AtomClass::Op),
    ("propto", '\u{221D}', AtomClass::Rel),
    ("psi", '\u{03C8}', AtomClass::Ord),
    ("rArr", '\u{21D2}', AtomClass::Rel),
    ("rVert", '\u{2016}', AtomClass::Close),
    ("rang", '\u{27E9}', AtomClass::Close),
    ("rangle", '\u{27E9}', AtomClass::Close),
    ("rarr", '\u{2192}', AtomClass::Rel),
    ("rbrace", '\u{007D}', AtomClass::Close),
    ("rbrack", '\u{005D}', AtomClass::Close),
    ("rceil", '\u{2309}', AtomClass::Close),
    ("real", '\u{211C}', AtomClass::Ord),
    ("reals", '\u{211D}', AtomClass::Ord),
    ("restriction", '\u{21BE}', AtomClass::Rel),
    ("rfloor", '\u{230B}', AtomClass::Close),
    ("rgroup", '\u{27EF}', AtomClass::Close),
    ("rhd", '\u{22B3}', AtomClass::Bin),
    ("rho", '\u{03C1}', AtomClass::Ord),
    ("rightarrow", '\u{2192}', AtomClass::Rel),
    ("rightarrowtail", '\u{21A3}', AtomClass::Rel),
    ("rightharpoondown", '\u{21C1}', AtomClass::Rel),
    ("rightharpoonup", '\u{21C0}', AtomClass::Rel),
    ("rightleftarrows", '\u{21C4}', AtomClass::Rel),
    ("rightleftharpoons", '\u{21CC}', AtomClass::Rel),
    ("rightrightarrows", '\u{21C9}', AtomClass::Rel),
    ("rightsquigarrow", '\u{21DD}', AtomClass::Rel),
    ("rightthreetimes", '\u{22CC}', AtomClass::Bin),
    ("risingdotseq", '\u{2253}', AtomClass::Rel),
    ("rmoustache", '\u{23B1}', AtomClass::Close),
    ("rtimes", '\u{22CA}', AtomClass::Bin),
    ("rvert", '\u{007C}', AtomClass::Close),
    ("sdot", '\u{22C5}', AtomClass::Bin),
    ("searrow", '\u{2198}', AtomClass::Rel),
    ("sect", '\u{00A7}', AtomClass::Ord),
    ("setminus", '\u{2216}', AtomClass::Bin),
    ("sharp", '\u{266F}', AtomClass::Ord),
    ("shortmid", '\u{2223}', AtomClass::Rel),
    ("shortparallel", '\u{2225}', AtomClass::Rel),
    ("sigma", '\u{03C3}', AtomClass::Ord),
    ("sim", '\u{223C}', AtomClass::Rel),
    ("simeq", '\u{2243}', AtomClass::Rel),
    ("smallfrown", '\u{2322}', AtomClass::Rel),
    ("smallsetminus", '\u{2216}', AtomClass::Bin),
    ("smallsmile", '\u{2323}', AtomClass::Rel),
    ("smile", '\u{2323}', AtomClass::Rel),
    ("spades", '\u{2660}', AtomClass::Ord),
    ("spadesuit", '\u{2660}', AtomClass::Ord),
    ("sphericalangle", '\u{2222}', AtomClass::Ord),
    ("sqcap", '\u{2293}', AtomClass::Bin),
    ("sqcup", '\u{2294}', AtomClass::Bin),
    ("sqsubset", '\u{228F}', AtomClass::Rel),
    ("sqsubseteq", '\u{2291}', AtomClass::Rel),
    ("sqsupset", '\u{2290}', AtomClass::Rel),
    ("sqsupseteq", '\u{2292}', AtomClass::Rel),
    ("square", '\u{25A1}', AtomClass::Ord),
    ("star", '\u{22C6}', AtomClass::Bin),
    ("sub", '\u{2282}', AtomClass::Rel),
    ("sube", '\u{2286}', AtomClass::Rel),
    ("subset", '\u{2282}', AtomClass::Rel),
    ("subseteq", '\u{2286}', AtomClass::Rel),
    ("subseteqq", '\u{2AC5}', AtomClass::Rel),
    ("subsetneq", '\u{228A}', AtomClass::Rel),
    ("subsetneqq", '\u{2ACB}', AtomClass::Rel),
    ("succ", '\u{227B}', AtomClass::Rel),
    ("succapprox", '\u{2AB8}', AtomClass::Rel),
    ("succcurlyeq", '\u{227D}', AtomClass::Rel),
    ("succeq", '\u{2AB0}', AtomClass::Rel),
    ("succnapprox", '\u{2ABA}', AtomClass::Rel),
    ("succneqq", '\u{2AB6}', AtomClass::Rel),
    ("succnsim", '\u{22E9}', AtomClass::Rel),
    ("succsim", '\u{227F}', AtomClass::Rel),
    ("sum", '\u{2211}', AtomClass::Op),
    ("supe", '\u{2287}', AtomClass::Rel),
    ("supset", '\u{2283}', AtomClass::Rel),
    ("supseteq", '\u{2287}', AtomClass::Rel),
    ("supseteqq", '\u{2AC6}', AtomClass::Rel),
    ("supsetneq", '\u{228B}', AtomClass::Rel),
    ("supsetneqq", '\u{2ACC}', AtomClass::Rel),
    ("surd", '\u{221A}', AtomClass::Ord),
    ("swarrow", '\u{2199}', AtomClass::Rel),
    ("tau", '\u{03C4}', AtomClass::Ord),
    ("therefore", '\u{2234}', AtomClass::Rel),
    ("theta", '\u{03B8}', AtomClass::Ord),
    ("thetasym", '\u{03D1}', AtomClass::Ord),
    ("thickapprox", '\u{2248}', AtomClass::Rel),
    ("thicksim", '\u{223C}', AtomClass::Rel),
    ("times", '\u{00D7}', AtomClass::Bin),
    ("to", '\u{2192}', AtomClass::Rel),
    ("top", '\u{22A4}', AtomClass::Ord),
    ("triangle", '\u{25B3}', AtomClass::Ord),
    ("triangledown", '\u{25BD}', AtomClass::Ord),
    ("triangleleft", '\u{25C3}', AtomClass::Bin),
    ("trianglelefteq", '\u{22B4}', AtomClass::Rel),
    ("triangleq", '\u{225C}', AtomClass::Rel),
    ("triangleright", '\u{25B9}', AtomClass::Bin),
    ("trianglerighteq", '\u{22B5}', AtomClass::Rel),
    ("twoheadleftarrow", '\u{219E}', AtomClass::Rel),
    ("twoheadrightarrow", '\u{21A0}', AtomClass::Rel),
    ("uArr", '\u{21D1}', AtomClass::Rel),
    ("uarr", '\u{2191}', AtomClass::Rel),
    ("ulcorner", '\u{231C}', AtomClass::Open),
    ("unlhd", '\u{22B4}', AtomClass::Bin),
    ("unrhd", '\u{22B5}', AtomClass::Bin),
    ("uparrow", '\u{2191}', AtomClass::Rel),
    ("updownarrow", '\u{2195}', AtomClass::Rel),
    ("upharpoonleft", '\u{21BF}', AtomClass::Rel),
    ("upharpoonright", '\u{21BE}', AtomClass::Rel),
    ("uplus", '\u{228E}', AtomClass::Bin),
    ("upsilon", '\u{03C5}', AtomClass::Ord),
    ("upuparrows", '\u{21C8}', AtomClass::Rel),
    ("urcorner", '\u{231D}', AtomClass::Close),
    ("vDash", '\u{22A8}', AtomClass::Rel),
    ("varDelta", '\u{1D6E5}', AtomClass::Ord),
    ("varGamma", '\u{1D6E4}', AtomClass::Ord),
    ("varLambda", '\u{1D6EC}', AtomClass::Ord),
    ("varOmega", '\u{1D6FA}', AtomClass::Ord),
    ("varPhi", '\u{1D6F7}', AtomClass::Ord),
    ("varPi", '\u{1D6F1}', AtomClass::Ord),
    ("varSigma", '\u{1D6F4}', AtomClass::Ord),
    ("varTheta", '\u{1D6E9}', AtomClass::Ord),
    ("varUpsilon", '\u{1D6F6}', AtomClass::Ord),
    ("varXi", '\u{1D6EF}', AtomClass::Ord),
    ("varepsilon", '\u{03B5}', AtomClass::Ord),
    ("varkappa", '\u{03F0}', AtomClass::Ord),
    ("varnothing", '\u{2205}', AtomClass::Ord),
    ("varphi", '\u{03C6}', AtomClass::Ord),
    ("varpi", '\u{03D6}', AtomClass::Ord),
    ("varpropto", '\u{221D}', AtomClass::Rel),
    ("varrho", '\u{03F1}', AtomClass::Ord),
    ("varsigma", '\u{03C2}', AtomClass::Ord),
    ("vartheta", '\u{03D1}', AtomClass::Ord),
    ("vartriangle", '\u{25B3}', AtomClass::Rel),
    ("vartriangleleft", '\u{22B2}', AtomClass::Rel),
    ("vartriangleright", '\u{22B3}', AtomClass::Rel),
    ("vdash", '\u{22A2}', AtomClass::Rel),
    ("vdots", '\u{22EE}', AtomClass::Ord),
    ("vee", '\u{2228}', AtomClass::Bin),
    ("veebar", '\u{22BB}', AtomClass::Bin),
    ("vert", '\u{007C}', AtomClass::Ord),
    ("wedge", '\u{2227}', AtomClass::Bin),
    ("weierp", '\u{2118}', AtomClass::Ord),
    ("wp", '\u{2118}', AtomClass::Ord),
    ("wr", '\u{2240}', AtomClass::Bin),
    ("xi", '\u{03BE}', AtomClass::Ord),
    ("zeta", '\u{03B6}', AtomClass::Ord),
    ("{", '{', AtomClass::Open),
    ("|", '\u{2016}', AtomClass::Ord),
    ("}", '}', AtomClass::Close),
];

/// Parses a TeX math string into a math list. Total: any input yields a
/// list, degraded where not understood.
pub fn parse(tex: &str) -> MathList {
    let mut parser = Parser {
        tokens: crate::token::tokenize(tex),
        pos: 0,
        src: tex,
        macros: std::collections::HashMap::new(),
        expansions: EXPANSION_CAP,
        spliced: 0,
    };
    parser.list(true)
}

/// Delimiter commands `\left` and `\right` accept, sorted for binary
/// search. Plain characters and `.` resolve without the table.
pub const DELIMITERS: &[(&str, char)] = &[
    ("Arrowvert", '\u{2016}'),
    ("Downarrow", '\u{21D3}'),
    ("Uparrow", '\u{21D1}'),
    ("Updownarrow", '\u{21D5}'),
    ("Vert", '\u{2016}'),
    ("arrowvert", '|'),
    ("backslash", '\\'),
    ("bracevert", '|'),
    ("downarrow", '\u{2193}'),
    ("gt", '\u{27E9}'),
    ("lVert", '\u{2016}'),
    ("lang", '\u{27E8}'),
    ("langle", '\u{27E8}'),
    ("lbrace", '{'),
    ("lbrack", '['),
    ("lceil", '\u{2308}'),
    ("lfloor", '\u{230A}'),
    ("lgroup", '\u{27EE}'),
    ("llcorner", '\u{231E}'),
    ("lmoustache", '\u{23B0}'),
    ("lrcorner", '\u{231F}'),
    ("lt", '\u{27E8}'),
    ("lvert", '|'),
    ("rVert", '\u{2016}'),
    ("rang", '\u{27E9}'),
    ("rangle", '\u{27E9}'),
    ("rbrace", '}'),
    ("rbrack", ']'),
    ("rceil", '\u{2309}'),
    ("rfloor", '\u{230B}'),
    ("rgroup", '\u{27EF}'),
    ("rmoustache", '\u{23B1}'),
    ("rvert", '|'),
    ("ulcorner", '\u{231C}'),
    ("uparrow", '\u{2191}'),
    ("updownarrow", '\u{2195}'),
    ("urcorner", '\u{231D}'),
    ("vert", '|'),
    ("{", '{'),
    ("|", '\u{2016}'),
    ("}", '}'),
];

fn vocabulary_lookup(name: &str) -> Option<(char, AtomClass)> {
    VOCABULARY
        .binary_search_by(|row| row.0.cmp(name))
        .ok()
        .map(|i| (VOCABULARY[i].1, VOCABULARY[i].2))
}

/// Accent commands: the combining character and whether wide forms may
/// stretch horizontally. Sorted for binary search.
pub const ACCENTS: &[(&str, char, bool)] = &[
    ("acute", '\u{0301}', false),
    ("bar", '\u{0304}', false),
    ("breve", '\u{0306}', false),
    ("check", '\u{030C}', false),
    ("ddddot", '\u{20DC}', false),
    ("dddot", '\u{20DB}', false),
    ("ddot", '\u{0308}', false),
    ("dot", '\u{0307}', false),
    ("grave", '\u{0300}', false),
    ("hat", '\u{0302}', false),
    ("mathring", '\u{030A}', false),
    ("overleftarrow", '\u{20D6}', true),
    ("overleftrightarrow", '\u{20E1}', true),
    ("overrightarrow", '\u{20D7}', true),
    ("tilde", '\u{0303}', false),
    ("vec", '\u{20D7}', false),
    ("widecheck", '\u{030C}', true),
    ("widehat", '\u{0302}', true),
    ("widetilde", '\u{0303}', true),
];

/// Operator names: upright Op atoms, flagged when TeX stacks their
/// limits in display style. Sorted for binary search.
pub const OP_NAMES: &[(&str, bool)] = &[
    ("Pr", true),
    ("arccos", false),
    ("arccot", false),
    ("arccsc", false),
    ("arcsec", false),
    ("arcsin", false),
    ("arctan", false),
    ("arg", false),
    ("argmax", true),
    ("argmin", true),
    ("cos", false),
    ("cosh", false),
    ("cot", false),
    ("coth", false),
    ("csc", false),
    ("csch", false),
    ("deg", false),
    ("det", true),
    ("dim", false),
    ("exp", false),
    ("gcd", true),
    ("hom", false),
    ("inf", true),
    ("injlim", true),
    ("ker", false),
    ("lg", false),
    ("lim", true),
    ("liminf", true),
    ("limsup", true),
    ("ln", false),
    ("log", false),
    ("max", true),
    ("min", true),
    ("plim", true),
    ("projlim", true),
    ("sec", false),
    ("sech", false),
    ("sen", false),
    ("sgn", false),
    ("sin", false),
    ("sinh", false),
    ("sup", true),
    ("tan", false),
    ("tanh", false),
];

/// The explicit spacing commands, in ems of the current style.
fn spacing_ems(name: &str) -> Option<f32> {
    Some(match name {
        "," => 3.0 / 18.0,
        ":" => 4.0 / 18.0,
        ";" => 5.0 / 18.0,
        "!" => -3.0 / 18.0,
        " " => 0.25,
        "quad" => 1.0,
        "qquad" => 2.0,
        // KaTeX's spacing macros.
        "enspace" => 0.5,
        "thinspace" => 3.0 / 18.0,
        "medspace" => 4.0 / 18.0,
        "thickspace" => 5.0 / 18.0,
        "negthinspace" => -3.0 / 18.0,
        "negmedspace" => -4.0 / 18.0,
        "negthickspace" => -5.0 / 18.0,
        "nobreakspace" | "space" => 0.25,
        _ => return None,
    })
}

/// One letter-style command's codepoint remap into the Mathematical
/// Alphanumeric block, Letterlike Symbols holes included. A character
/// outside the command's alphabet stays itself.
pub(crate) fn map_alphabet(name: &str, c: char) -> char {
    let a = c as u32;
    let mapped = match name {
        "mathbb" => match c {
            'C' => 0x2102,
            'H' => 0x210D,
            'N' => 0x2115,
            'P' => 0x2119,
            'Q' => 0x211A,
            'R' => 0x211D,
            'Z' => 0x2124,
            'A'..='Z' => 0x1D538 + (a - 'A' as u32),
            'a'..='z' => 0x1D552 + (a - 'a' as u32),
            '0'..='9' => 0x1D7D8 + (a - '0' as u32),
            _ => a,
        },
        "mathbf" => match c {
            'A'..='Z' => 0x1D400 + (a - 'A' as u32),
            'a'..='z' => 0x1D41A + (a - 'a' as u32),
            '0'..='9' => 0x1D7CE + (a - '0' as u32),
            '\u{0391}'..='\u{03A9}' => 0x1D6A8 + (a - 0x0391),
            '\u{03B1}'..='\u{03C9}' => 0x1D6C2 + (a - 0x03B1),
            _ => a,
        },
        "mathit" => match c {
            'h' => 0x210E,
            'A'..='Z' => 0x1D434 + (a - 'A' as u32),
            'a'..='z' => 0x1D44E + (a - 'a' as u32),
            _ => a,
        },
        // `\boldsymbol`: bold italic letters and small Greek, bold digits,
        // and bold upright Greek capitals, upright as `\Gamma` is.
        "boldsymbol" => match c {
            'A'..='Z' => 0x1D468 + (a - 'A' as u32),
            'a'..='z' => 0x1D482 + (a - 'a' as u32),
            '0'..='9' => 0x1D7CE + (a - '0' as u32),
            '\u{0391}'..='\u{03A9}' => 0x1D6A8 + (a - 0x0391),
            '\u{03B1}'..='\u{03C9}' => 0x1D736 + (a - 0x03B1),
            '\u{2207}' => 0x1D735,
            '\u{2202}' => 0x1D74F,
            _ => a,
        },
        "mathcal" => match c {
            'B' => 0x212C,
            'E' => 0x2130,
            'F' => 0x2131,
            'H' => 0x210B,
            'I' => 0x2110,
            'L' => 0x2112,
            'M' => 0x2133,
            'R' => 0x211B,
            'e' => 0x212F,
            'g' => 0x210A,
            'o' => 0x2134,
            'A'..='Z' => 0x1D49C + (a - 'A' as u32),
            'a'..='z' => 0x1D4B6 + (a - 'a' as u32),
            _ => a,
        },
        "mathfrak" => match c {
            'C' => 0x212D,
            'H' => 0x210C,
            'I' => 0x2111,
            'R' => 0x211C,
            'Z' => 0x2128,
            'A'..='Z' => 0x1D504 + (a - 'A' as u32),
            'a'..='z' => 0x1D51E + (a - 'a' as u32),
            _ => a,
        },
        "mathsf" => match c {
            'A'..='Z' => 0x1D5A0 + (a - 'A' as u32),
            'a'..='z' => 0x1D5BA + (a - 'a' as u32),
            '0'..='9' => 0x1D7E2 + (a - '0' as u32),
            _ => a,
        },
        "mathtt" => match c {
            'A'..='Z' => 0x1D670 + (a - 'A' as u32),
            'a'..='z' => 0x1D68A + (a - 'a' as u32),
            '0'..='9' => 0x1D7F6 + (a - '0' as u32),
            _ => a,
        },
        _ => a,
    };
    char::from_u32(mapped).unwrap_or(c)
}

/// Applies a letter-style remap through a list: symbols map, every
/// nested field recurses, literals and text stay themselves.
fn restyle(list: &mut MathList, name: &str) {
    for noad in &mut list.0 {
        if let Noad::Atom(atom) = noad {
            restyle_atom(atom, name);
        }
    }
}

/// One atom restyled: its nucleus, then its scripts.
fn restyle_atom(atom: &mut Atom, name: &str) {
    restyle_field(&mut atom.nucleus, name);
    if let Some(s) = &mut atom.sup {
        restyle(s, name);
    }
    if let Some(s) = &mut atom.sub {
        restyle(s, name);
    }
}

fn restyle_field(field: &mut Field, name: &str) {
    // `\rm`: a Latin letter becomes an upright text atom, as `\mathrm x`
    // reads one; the other symbols have no roman form to take.
    if let Field::Symbol(c) = field {
        if name == "mathrm" && c.is_ascii_alphabetic() {
            let text = c.to_string();
            *field = Field::Text(text);
            return;
        }
    }
    match field {
        Field::Symbol(c) => *c = map_alphabet(name, *c),
        Field::List(inner) => restyle(inner, name),
        Field::Fraction {
            numerator,
            denominator,
            ..
        } => {
            restyle(numerator, name);
            restyle(denominator, name);
        }
        Field::Radical { radicand, degree } => {
            restyle(radicand, name);
            if let Some(deg) = degree {
                restyle(deg, name);
            }
        }
        Field::LeftRight { body, .. } => restyle(body, name),
        Field::Accent { base, .. } => restyle(base, name),
        Field::Table { rows, .. } => {
            for row in rows {
                for cell in row {
                    restyle(cell, name);
                }
            }
        }
        Field::Text(_)
        | Field::Literal(_)
        | Field::Kern(_)
        | Field::Sized { .. }
        | Field::Empty => {}
    }
}

/// The text of a `\text{...}` group, spaces included: each token's own
/// text, and between two tokens the whitespace of the source between
/// their places. A token a macro spliced in points at the definition's
/// body, so a text written in a body reads where it is written; the
/// arguments a macro mixed into its body point at the call, and around
/// them the body's whitespace is kept. Between two arguments nothing is
/// kept, their braces are not part of the text.
fn text_between(
    src: &str,
    open: &crate::token::Token,
    content: &[crate::token::Token],
    close: Option<&crate::token::Token>,
) -> String {
    use crate::token::TokenKind as K;
    let place = |t: &crate::token::Token| t.origin.clone().unwrap_or_else(|| t.span.clone());
    let start = place(open).end;
    let end = close.map(|t| place(t).start);
    // The group's own tokens lie between its braces; any other place is
    // a macro's argument.
    let own = |r: &std::ops::Range<usize>| r.start >= start && end.map_or(true, |e| r.end <= e);
    let mut out = String::new();
    let mut cursor = start;
    let mut prev_own = true;
    for t in content {
        let r = place(t);
        let is_own = own(&r);
        out.push_str(&text_gap(src, cursor, prev_own, r.start, is_own));
        match &t.kind {
            K::Char(c) => out.push(*c),
            K::Command(name) => {
                out.push('\\');
                out.push_str(name);
            }
            K::BeginGroup => out.push('{'),
            K::EndGroup => out.push('}'),
            K::Sup => out.push('^'),
            K::Sub => out.push('_'),
            K::Align => out.push('&'),
            K::Prime => out.push('\''),
        }
        cursor = r.end;
        prev_own = is_own;
    }
    if let Some(end) = end {
        out.push_str(&text_gap(src, cursor, prev_own, end, true));
    }
    out
}

/// The whitespace of a text between two of its tokens, the first ending
/// at `a` and the second starting at `b` in the source, each in the
/// group's own braces or in a macro's argument.
fn text_gap(src: &str, a: usize, a_own: bool, b: usize, b_own: bool) -> String {
    let ws = |c: char| c.is_whitespace();
    match (a_own, b_own) {
        // Both in the group: the source between them, without the `#k`
        // of an argument that expanded to nothing.
        (true, true) if a <= b => {
            let gap = src.get(a..b).unwrap_or("");
            let mut out = String::with_capacity(gap.len());
            let mut chars = gap.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '#' && chars.peek().is_some_and(|d| d.is_ascii_digit()) {
                    chars.next();
                    continue;
                }
                out.push(c);
            }
            out
        }
        // From the group to an argument: the whitespace before its `#k`.
        (true, false) => {
            let after = src.get(a..).unwrap_or("");
            after[..after.len() - after.trim_start_matches(ws).len()].to_string()
        }
        // From an argument back to the group: the whitespace after the `#k`.
        (false, true) => {
            let before = src.get(..b).unwrap_or("");
            before[before.trim_end_matches(ws).len()..].to_string()
        }
        // Inside one argument, its own whitespace; between two arguments,
        // their braces, which are not part of the text.
        (false, false) if a <= b => {
            let gap = src.get(a..b).unwrap_or("");
            if gap.chars().all(ws) {
                gap.to_string()
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

/// What a token is as a delimiter: `Some(None)` for the dot, TeX's empty
/// delimiter; `Some(Some(c))` for TeX's delimiter characters (`<` and `>`
/// as the angle brackets, as TeX's delimiter codes map them), for a
/// character one of the delimiter commands stands for, and for a command
/// of `DELIMITERS`; `None` for anything else, a letter among them, which
/// TeX refuses as a delimiter.
fn delimiter_of(kind: &crate::token::TokenKind) -> Option<Option<char>> {
    use crate::token::TokenKind as K;
    match kind {
        K::Char('.') => Some(None),
        K::Char('<') => Some(Some('\u{27E8}')),
        K::Char('>') => Some(Some('\u{27E9}')),
        K::Char(c)
            if matches!(c, '(' | ')' | '[' | ']' | '/' | '|')
                || DELIMITERS.iter().any(|row| row.1 == *c) =>
        {
            Some(Some(*c))
        }
        K::Command(name) => DELIMITERS
            .binary_search_by(|row| row.0.cmp(name.as_str()))
            .ok()
            .map(|i| Some(DELIMITERS[i].1)),
        _ => None,
    }
}

fn classify_char(c: char) -> AtomClass {
    match c {
        '+' | '\u{2212}' | '-' | '*' => AtomClass::Bin,
        '=' | '<' | '>' | ':' => AtomClass::Rel,
        '(' | '[' => AtomClass::Open,
        ')' | ']' => AtomClass::Close,
        ',' | ';' => AtomClass::Punct,
        _ => AtomClass::Ord,
    }
}

fn literal_atom(text: impl Into<String>, span: std::ops::Range<usize>) -> Atom {
    Atom {
        class: AtomClass::Ord,
        nucleus: Field::Literal(text.into()),
        sup: None,
        sub: None,
        limits: Limits::default(),
        span: span.clone(),
        nucleus_span: span,
    }
}

/// The items of one group as they are read: the atoms, and TeX's infix
/// words and switches as markers, which act when the group closes. The
/// four readers of a group share it through `read_item`: the top level
/// and a braced group (`list`), the body of `\left` (`left_right`), a
/// table cell (`table_cells`) and a root's degree without its bracket
/// (`list_until_char`).
#[derive(Default)]
struct Group {
    items: Vec<Item>,
}

enum Item {
    Atom(Atom),
    Marker(Marker, std::ops::Range<usize>),
}

/// A word that acts on the group it stands in.
#[derive(Clone, Copy)]
enum Marker {
    Infix(Infix),
    /// The alphabet of `map_alphabet`, `mathrm` for `\rm`.
    Font(&'static str),
    Style(MathStyle),
}

/// TeX's infix fraction words.
#[derive(Clone, Copy)]
enum Infix {
    Over,
    Atop,
    Choose,
    Brace,
    Brack,
}

impl Infix {
    fn name(self) -> &'static str {
        match self {
            Infix::Over => "\\over",
            Infix::Atop => "\\atop",
            Infix::Choose => "\\choose",
            Infix::Brace => "\\brace",
            Infix::Brack => "\\brack",
        }
    }
}

impl Marker {
    /// The marker a command name is, if any.
    fn of(name: &str) -> Option<Marker> {
        Some(match name {
            "over" => Marker::Infix(Infix::Over),
            "atop" => Marker::Infix(Infix::Atop),
            "choose" => Marker::Infix(Infix::Choose),
            "brace" => Marker::Infix(Infix::Brace),
            "brack" => Marker::Infix(Infix::Brack),
            "bf" => Marker::Font("mathbf"),
            "rm" => Marker::Font("mathrm"),
            "it" => Marker::Font("mathit"),
            "sf" => Marker::Font("mathsf"),
            "tt" => Marker::Font("mathtt"),
            "cal" => Marker::Font("mathcal"),
            "displaystyle" => Marker::Style(MathStyle::Display),
            "textstyle" => Marker::Style(MathStyle::Text),
            "scriptstyle" => Marker::Style(MathStyle::Script),
            "scriptscriptstyle" => Marker::Style(MathStyle::ScriptScript),
            _ => return None,
        })
    }
}

impl Group {
    fn push(&mut self, atom: Atom) {
        self.items.push(Item::Atom(atom));
    }

    fn mark(&mut self, marker: Marker, span: std::ops::Range<usize>) {
        self.items.push(Item::Marker(marker, span));
    }

    /// The atom a script marker hangs on when it follows a font switch:
    /// TeX's font is a state, not a noad, so the atom before the switch
    /// takes the script. After a style switch or an infix word, none.
    fn script_target(&mut self) -> Option<&mut Atom> {
        let last = self
            .items
            .iter()
            .rposition(|item| !matches!(item, Item::Marker(Marker::Font(_), _)))?;
        match &mut self.items[last] {
            Item::Atom(atom) => Some(atom),
            Item::Marker(..) => None,
        }
    }

    /// The group's list with its markers applied, TeX's way: a font switch
    /// holds to the end of the group, an infix word included; the first
    /// infix word splits the group into a fraction, and a later one is a
    /// literal in the denominator, where TeX refuses the formula
    /// ("Ambiguous"); a style switch is a noad of the list it stands in,
    /// the numerator's when it comes before the word. TeX's binary rules
    /// run on each list built.
    fn finish(self) -> MathList {
        let mut items = self.items;
        let mut font: Option<&'static str> = None;
        for item in &mut items {
            match item {
                Item::Marker(Marker::Font(alphabet), _) => font = Some(alphabet),
                Item::Atom(atom) => {
                    if let Some(alphabet) = font {
                        restyle_atom(atom, alphabet);
                    }
                }
                Item::Marker(..) => {}
            }
        }
        let infix = items
            .iter()
            .position(|item| matches!(item, Item::Marker(Marker::Infix(_), _)));
        let Some(at) = infix else {
            let mut noads = noads_of(items);
            demote_bins(&mut noads);
            return MathList(noads);
        };
        // The fraction spans every item, whatever their order in the
        // source (a macro's arguments may come swapped).
        let (start, end) = items
            .iter()
            .map(item_span)
            .fold((usize::MAX, 0), |(s, e), r| (s.min(r.start), e.max(r.end)));
        let span = start..end;
        let mut tail = items.split_off(at + 1);
        let Some(Item::Marker(Marker::Infix(infix), _)) = items.pop() else {
            unreachable!("the infix word found")
        };
        for item in &mut tail {
            if let Item::Marker(Marker::Infix(second), span) = item {
                *item = Item::Atom(literal_atom(second.name(), span.clone()));
            }
        }
        let mut numerator = noads_of(items);
        demote_bins(&mut numerator);
        let mut denominator = noads_of(tail);
        demote_bins(&mut denominator);
        let atom = infix_atom(infix, MathList(numerator), MathList(denominator), span);
        MathList(vec![Noad::Atom(atom)])
    }
}

fn item_span(item: &Item) -> std::ops::Range<usize> {
    match item {
        Item::Atom(atom) => atom.span.clone(),
        Item::Marker(_, span) => span.clone(),
    }
}

/// The noads of a group's items: its atoms, and a style noad for each
/// style switch. The font switches have acted, and the infix words have
/// been taken or made literals.
fn noads_of(items: Vec<Item>) -> Vec<Noad> {
    items
        .into_iter()
        .filter_map(|item| match item {
            Item::Atom(atom) => Some(Noad::Atom(atom)),
            Item::Marker(Marker::Style(style), _) => Some(Noad::Style(style)),
            Item::Marker(Marker::Font(_), _) => None,
            Item::Marker(Marker::Infix(_), _) => unreachable!("the infix words were taken"),
        })
        .collect()
}

/// The fraction an infix word makes of its group's two halves: `\over`
/// with a bar, `\atop` without, `\choose`, `\brace` and `\brack` the
/// barless stack between their stretched delimiters, as `\binom`.
fn infix_atom(
    infix: Infix,
    numerator: MathList,
    denominator: MathList,
    span: std::ops::Range<usize>,
) -> Atom {
    match infix {
        Infix::Over | Infix::Atop => Atom {
            class: AtomClass::Inner,
            nucleus: Field::Fraction {
                numerator,
                denominator,
                bar: matches!(infix, Infix::Over),
                style: None,
                continued: false,
            },
            sup: None,
            sub: None,
            limits: Limits::default(),
            span: span.clone(),
            nucleus_span: span,
        },
        Infix::Choose => binom_atom(numerator, denominator, None, span, ('(', ')')),
        Infix::Brace => binom_atom(numerator, denominator, None, span, ('{', '}')),
        Infix::Brack => binom_atom(numerator, denominator, None, span, ('[', ']')),
    }
}

/// A barless stack inside stretched delimiters: `\binom` and the infix
/// `\choose` family.
fn binom_atom(
    numerator: MathList,
    denominator: MathList,
    style: Option<MathStyle>,
    span: std::ops::Range<usize>,
    delimiters: (char, char),
) -> Atom {
    let stack = Atom {
        class: AtomClass::Inner,
        nucleus: Field::Fraction {
            numerator,
            denominator,
            bar: false,
            style,
            continued: false,
        },
        sup: None,
        sub: None,
        limits: Limits::default(),
        span: span.clone(),
        nucleus_span: span.clone(),
    };
    Atom {
        class: AtomClass::Inner,
        nucleus: Field::LeftRight {
            open: Some(delimiters.0),
            close: Some(delimiters.1),
            body: MathList(vec![Noad::Atom(stack)]),
        },
        sup: None,
        sub: None,
        limits: Limits::default(),
        span: span.clone(),
        nucleus_span: span,
    }
}

/// TeX's rules for a binary atom: with no quantity on its left, or none on
/// its right, it reads as an ordinary symbol, so leading signs, doubled
/// operators and a sign before a closer space as signs. Applies per list;
/// groups and scripts recurse through `list`.
fn demote_bins(items: &mut [Noad]) {
    // Kerns and style noads are not atoms: demotion reads through them.
    let is_atom =
        |noad: &Noad| matches!(noad, Noad::Atom(atom) if !matches!(atom.nucleus, Field::Kern(_)));
    let mut prev: Option<AtomClass> = None;
    for i in 0..items.len() {
        if !is_atom(&items[i]) {
            continue;
        }
        // TeX's rules 5 and 6: a binary operator is ordinary after nothing,
        // after another Bin, an Op, a Rel, an Open or a Punct, and before
        // nothing, a Rel, a Close or a Punct.
        let next = items[i + 1..]
            .iter()
            .find(|n| is_atom(n))
            .and_then(|n| match n {
                Noad::Atom(atom) => Some(atom.class),
                Noad::Style(_) => None,
            });
        let Noad::Atom(atom) = &mut items[i] else {
            unreachable!("an atom")
        };
        if atom.class == AtomClass::Bin
            && (!matches!(
                prev,
                Some(AtomClass::Ord) | Some(AtomClass::Close) | Some(AtomClass::Inner)
            ) || matches!(
                next,
                None | Some(AtomClass::Rel) | Some(AtomClass::Close) | Some(AtomClass::Punct)
            ))
        {
            atom.class = AtomClass::Ord;
        }
        prev = Some(atom.class);
    }
}

/// Macro expansion budgets: calls expanded per parse, and total tokens
/// spliced across them. Hostile definitions exhaust a budget and every
/// later call degrades to a literal; both bounds keep the parse linear
/// in the source.
const EXPANSION_CAP: usize = 256;
const SPLICE_CEILING: usize = 50_000;

/// A document-defined macro: `\newcommand`'s parameter count, the
/// optional default for `#1`, and the body tokens as captured.
#[derive(Clone)]
struct Macro {
    params: usize,
    default: Option<Vec<crate::token::Token>>,
    body: Vec<crate::token::Token>,
}

struct Parser<'a> {
    tokens: Vec<crate::token::Token>,
    pos: usize,
    src: &'a str,
    macros: std::collections::HashMap<String, Macro>,
    expansions: usize,
    spliced: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&crate::token::Token> {
        self.tokens.get(self.pos)
    }

    /// A marker word at the stream's head; a user macro of that name is
    /// not one, it expands as an atom.
    fn marker(&self) -> Option<Marker> {
        use crate::token::TokenKind as K;
        let K::Command(name) = &self.peek()?.kind else {
            return None;
        };
        if self.macros.contains_key(name) {
            return None;
        }
        Marker::of(name)
    }

    /// One item of a group from the stream into the collector: a marker
    /// word; a script with nothing to hang on, which lands on the atom
    /// before a font switch (TeX's font is no noad) and on an empty
    /// nucleus otherwise (TeX's implicit empty group); an alignment
    /// marker outside a table, a literal; or an atom with its scripts.
    /// Answers whether the stream moved: it does not at its end or at a
    /// group's closer, which the caller reads.
    fn read_item(&mut self, group: &mut Group) -> bool {
        use crate::token::TokenKind as K;
        let Some(tok) = self.peek() else {
            return false;
        };
        let span = tok.span.clone();
        match &tok.kind {
            K::EndGroup => false,
            K::Align => {
                self.pos += 1;
                group.push(literal_atom("&", span));
                true
            }
            K::Sup | K::Sub | K::Prime => {
                if let Some(atom) = group.script_target() {
                    self.scripts(atom);
                } else {
                    let mut atom = Atom {
                        class: AtomClass::Ord,
                        nucleus: Field::Empty,
                        sup: None,
                        sub: None,
                        limits: Limits::default(),
                        span: span.start..span.start,
                        nucleus_span: span.start..span.start,
                    };
                    self.scripts(&mut atom);
                    group.push(atom);
                }
                true
            }
            _ => {
                if let Some(marker) = self.marker() {
                    self.pos += 1;
                    group.mark(marker, span);
                    return true;
                }
                let before = self.pos;
                match self.atom() {
                    Some(mut atom) => {
                        self.scripts(&mut atom);
                        group.push(atom);
                        true
                    }
                    None => self.pos != before,
                }
            }
        }
    }

    fn next(&mut self) -> Option<crate::token::Token> {
        let tok = self.tokens.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    /// Parses noads until end of input, or until the matching group closer
    /// when `top` is false. A stray closer at top level degrades to a
    /// literal; a missing closer closes at the end.
    fn list(&mut self, top: bool) -> MathList {
        use crate::token::TokenKind as K;
        let mut group = Group::default();
        while let Some(tok) = self.peek() {
            let span = tok.span.clone();
            match &tok.kind {
                K::EndGroup => {
                    self.pos += 1;
                    if !top {
                        break;
                    }
                    group.push(literal_atom("}", span));
                }
                K::Align => {
                    self.pos += 1;
                    group.push(literal_atom("&", span));
                }
                _ => {
                    if !self.read_item(&mut group) {
                        break;
                    }
                }
            }
        }
        group.finish()
    }

    /// One scriptless atom from the stream: a character, a command, or a
    /// braced group. The caller has excluded every other token kind.
    /// Definitions and macro calls resolve here first, transparent to
    /// every construct that reads atoms; whenever the stream is nonempty
    /// the call consumes at least one token.
    fn atom(&mut self) -> Option<Atom> {
        use crate::token::TokenKind as K;
        let mut expanded = false;
        let tok = loop {
            let tok = self.next()?;
            if let K::Command(name) = &tok.kind {
                if name == "newcommand" || name == "renewcommand" {
                    if self.define().is_ok() {
                        continue;
                    }
                    let end = self.consumed_end(tok.span.end);
                    let span = tok.span.start..end;
                    let text = self.src.get(span.clone()).unwrap_or("\\newcommand");
                    return Some(literal_atom(text.to_string(), span));
                }
                if self.macros.contains_key(name.as_str()) {
                    let name = name.clone();
                    if self.expand(&name, tok.span.clone()) {
                        expanded = true;
                        continue;
                    }
                    return Some(literal_atom(format!("\\{name}"), tok.span));
                }
                // A closer met after a macro expanded to nothing (a row
                // break, `\\right`, `\\end`): the reader of the structure
                // takes it, as it would have without the macro.
                if expanded && (name == "\\" || name == "right" || name == "end") {
                    self.pos -= 1;
                    return None;
                }
                // A word that acts on the group (`\\over`, a switch) is no
                // atom: the group's reader takes it, after a definition or
                // an expansion as at the head of the stream.
                if Marker::of(name).is_some() {
                    self.pos -= 1;
                    return None;
                }
            }
            break tok;
        };
        match tok.kind {
            // TeX's tie is a space in a formula, the width of `\ `.
            K::Char('~') => Some(Atom {
                class: AtomClass::Ord,
                nucleus: Field::Kern(0.25),
                sup: None,
                sub: None,
                limits: Limits::default(),
                span: tok.span.clone(),
                nucleus_span: tok.span,
            }),
            K::Char(c) => {
                // Math mode's hyphen is the minus sign.
                let c = if c == '-' { '\u{2212}' } else { c };
                Some(Atom {
                    class: classify_char(c),
                    nucleus: Field::Symbol(c),
                    sup: None,
                    sub: None,
                    limits: Limits::default(),
                    span: tok.span.clone(),
                    nucleus_span: tok.span,
                })
            }
            K::Command(name) => Some(match name.as_str() {
                "frac" => self.fraction(tok.span, true, None, false),
                "tfrac" => self.fraction(tok.span, true, Some(MathStyle::Text), false),
                "dfrac" => self.fraction(tok.span, true, Some(MathStyle::Display), false),
                "cfrac" => self.fraction(tok.span, true, Some(MathStyle::Display), true),
                "binom" => self.binom(tok.span, None),
                "tbinom" => self.binom(tok.span, Some(MathStyle::Text)),
                "dbinom" => self.binom(tok.span, Some(MathStyle::Display)),
                "sqrt" => self.radical(tok.span),
                "left" => self.left_right(tok.span),
                "right" => {
                    // A stray closer: its delimiter goes with it, the pair
                    // degrades to a literal.
                    let _ = self.delimiter();
                    literal_atom("\\right", tok.span)
                }
                "text" | "mathrm" | "textrm" | "textnormal" | "textup" | "hbox" | "mbox" => {
                    self.text(tok.span, &name)
                }
                "pmod" => self.modulo(tok.span, true),
                "mod" => self.modulo(tok.span, false),
                "big" | "Big" | "bigg" | "Bigg" | "bigl" | "Bigl" | "biggl" | "Biggl" | "bigr"
                | "Bigr" | "biggr" | "Biggr" | "bigm" | "Bigm" | "biggm" | "Biggm" => {
                    self.sized(tok.span, &name)
                }
                "mathop" => self.classed(tok.span, AtomClass::Op),
                "mathrel" => self.classed(tok.span, AtomClass::Rel),
                "mathbin" => self.classed(tok.span, AtomClass::Bin),
                "mathord" => self.classed(tok.span, AtomClass::Ord),
                "mathopen" => self.classed(tok.span, AtomClass::Open),
                "mathclose" => self.classed(tok.span, AtomClass::Close),
                "mathpunct" => self.classed(tok.span, AtomClass::Punct),
                "mathinner" => self.classed(tok.span, AtomClass::Inner),
                "operatorname" => {
                    // The starred form stacks its display limits.
                    let star = matches!(self.peek().map(|t| &t.kind), Some(K::Char('*')));
                    if star {
                        self.pos += 1;
                    }
                    let mut atom = self.text(tok.span, &name);
                    if matches!(atom.nucleus, Field::Text(_)) {
                        atom.class = AtomClass::Op;
                        atom.limits = if star {
                            Limits::Default
                        } else {
                            Limits::NoLimits
                        };
                    }
                    atom
                }
                "bmod" => Atom {
                    class: AtomClass::Bin,
                    nucleus: Field::Text("mod".into()),
                    sup: None,
                    sub: None,
                    limits: Limits::NoLimits,
                    span: tok.span.clone(),
                    nucleus_span: tok.span,
                },
                "begin" => self.environment(tok.span),
                "end" => {
                    // A stray closer: its name goes with it, the pair
                    // degrades to a literal.
                    let _ = self.env_name();
                    let end = self.consumed_end(tok.span.end);
                    let span = tok.span.start..end;
                    literal_atom(
                        self.src.get(span.clone()).unwrap_or("\\end").to_string(),
                        span,
                    )
                }
                "mathbb" | "mathbf" | "mathcal" | "mathfrak" | "mathit" | "mathsf" | "mathtt"
                | "boldsymbol" | "mathnormal" => self.styled(tok.span, &name),
                // The other names of the alphabets.
                "bm" => self.styled(tok.span, "boldsymbol"),
                "Bbb" => self.styled(tok.span, "mathbb"),
                "bold" => self.styled(tok.span, "mathbf"),
                "frak" => self.styled(tok.span, "mathfrak"),
                "mathscr" | "scr" => self.styled(tok.span, "mathcal"),
                _ => {
                    if let Some(ems) = spacing_ems(&name) {
                        Atom {
                            class: AtomClass::Ord,
                            nucleus: Field::Kern(ems),
                            sup: None,
                            sub: None,
                            limits: Limits::default(),
                            span: tok.span.clone(),
                            nucleus_span: tok.span,
                        }
                    } else if let Ok(i) = ACCENTS.binary_search_by(|row| row.0.cmp(name.as_str())) {
                        let (_, accent, stretch) = ACCENTS[i];
                        self.accent(tok.span, accent, stretch)
                    } else if let Ok(i) = OP_NAMES.binary_search_by(|row| row.0.cmp(name.as_str()))
                    {
                        Atom {
                            class: AtomClass::Op,
                            nucleus: Field::Text(name.clone()),
                            sup: None,
                            sub: None,
                            limits: if OP_NAMES[i].1 {
                                Limits::Default
                            } else {
                                Limits::NoLimits
                            },
                            span: tok.span.clone(),
                            nucleus_span: tok.span,
                        }
                    } else {
                        match vocabulary_lookup(&name) {
                            Some((ch, class)) => Atom {
                                class,
                                nucleus: Field::Symbol(ch),
                                sup: None,
                                sub: None,
                                limits: Limits::default(),
                                span: tok.span.clone(),
                                nucleus_span: tok.span,
                            },
                            None => literal_atom(format!("\\{name}"), tok.span),
                        }
                    }
                }
            }),
            K::BeginGroup => {
                let inner = self.list(false);
                let end = self
                    .tokens
                    .get(self.pos.saturating_sub(1))
                    .map(|t| t.span.end)
                    .unwrap_or(tok.span.end);
                Some(Atom {
                    class: AtomClass::Ord,
                    nucleus: Field::List(inner),
                    sup: None,
                    sub: None,
                    limits: Limits::default(),
                    span: tok.span.start..end,
                    nucleus_span: tok.span.start..end,
                })
            }
            // A marker is no atom: it stays for the caller, which closes its
            // group on it or reads an empty operand (a macro that expanded
            // to nothing stood before it).
            _ => {
                self.pos -= 1;
                None
            }
        }
    }

    /// A construct atom covering `start` through everything consumed since.
    fn construct(
        &mut self,
        start: std::ops::Range<usize>,
        nucleus: Field,
        class: AtomClass,
    ) -> Atom {
        let end = self.consumed_end(start.end);
        Atom {
            class,
            nucleus,
            sup: None,
            sub: None,
            limits: Limits::default(),
            span: start.start..end,
            nucleus_span: start.start..end,
        }
    }

    /// `\frac{num}{den}` and its variants; the argument reader accepts
    /// single tokens the way TeX does, so `\frac12` works. `\tfrac` and
    /// `\dfrac` force their style, `\cfrac` is continued: both are the
    /// layout's to honor.
    fn fraction(
        &mut self,
        start: std::ops::Range<usize>,
        bar: bool,
        style: Option<MathStyle>,
        continued: bool,
    ) -> Atom {
        let numerator = self.script_operand();
        let denominator = self.script_operand();
        self.construct(
            start,
            Field::Fraction {
                numerator,
                denominator,
                bar,
                style,
                continued,
            },
            AtomClass::Inner,
        )
    }

    /// `\binom{n}{k}`: a barless stack inside stretched parentheses;
    /// `\tbinom` and `\dbinom` force the stack's style.
    fn binom(&mut self, start: std::ops::Range<usize>, style: Option<MathStyle>) -> Atom {
        let numerator = self.script_operand();
        let denominator = self.script_operand();
        let end = self.consumed_end(start.end);
        binom_atom(numerator, denominator, style, start.start..end, ('(', ')'))
    }

    /// A letter-style command: the operand parses normally, then its
    /// symbols remap into the command's alphabet. A single restyled atom
    /// keeps its own class; a longer operand wraps as a group.
    fn styled(&mut self, start: std::ops::Range<usize>, name: &str) -> Atom {
        let mut operand = self.script_operand();
        restyle(&mut operand, name);
        let end = self.consumed_end(start.end);
        let span = start.start..end;
        if let [Noad::Atom(_)] = operand.0.as_slice() {
            let Some(Noad::Atom(mut atom)) = operand.0.pop() else {
                unreachable!("one atom")
            };
            atom.span = span.clone();
            atom.nucleus_span = span;
            atom
        } else {
            Atom {
                class: AtomClass::Ord,
                nucleus: Field::List(operand),
                sup: None,
                sub: None,
                limits: Limits::default(),
                span: span.clone(),
                nucleus_span: span,
            }
        }
    }

    /// An accent command over its operand.
    fn accent(&mut self, start: std::ops::Range<usize>, accent: char, stretch: bool) -> Atom {
        let base = self.script_operand();
        self.construct(
            start,
            Field::Accent {
                accent,
                stretch,
                base,
            },
            AtomClass::Ord,
        )
    }

    /// `\text{...}` and its upright kin: the braced source verbatim,
    /// spaces and nested braces included, which the tokens' places in
    /// the source recover, in a macro's body as at the top level, a tie
    /// inside it a space and `\~` kept.
    /// Without a group the argument is the next token, TeX's reading: a
    /// character as itself, a tie as a space, a symbol command as its
    /// character, a macro expanded first. Any other token leaves the
    /// command a literal under its own name.
    fn text(&mut self, start: std::ops::Range<usize>, name: &str) -> Atom {
        use crate::token::TokenKind as K;
        match self.peek().map(|t| t.kind.clone()) {
            Some(K::BeginGroup) => {}
            Some(K::Char('~')) => {
                self.pos += 1;
                return self.construct(start, Field::Text(" ".into()), AtomClass::Ord);
            }
            Some(K::Char(c)) => {
                self.pos += 1;
                return self.construct(start, Field::Text(c.to_string()), AtomClass::Ord);
            }
            Some(K::Command(command)) => {
                if self.macros.contains_key(&command) {
                    // `expand` works on a consumed macro token.
                    let span = self.peek().map(|t| t.span.clone()).unwrap_or(start.clone());
                    self.pos += 1;
                    if self.expand(&command, span) {
                        return self.text(start, name);
                    }
                    self.pos -= 1;
                    return literal_atom(format!("\\{name}"), start);
                }
                let Some((ch, _)) = vocabulary_lookup(&command) else {
                    return literal_atom(format!("\\{name}"), start);
                };
                self.pos += 1;
                return self.construct(start, Field::Text(ch.to_string()), AtomClass::Ord);
            }
            _ => return literal_atom(format!("\\{name}"), start),
        }
        let open = self.next().expect("peeked");
        let mut depth = 1usize;
        let mut content: Vec<crate::token::Token> = Vec::new();
        let mut close = None;
        while let Some(tok) = self.next() {
            match tok.kind {
                K::BeginGroup => depth += 1,
                K::EndGroup => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(tok);
                        break;
                    }
                }
                _ => {}
            }
            content.push(tok);
        }
        let raw = text_between(self.src, &open, &content, close.as_ref());
        // TeX's tie is a space inside text as well; `\~`, the tilde
        // accent, keeps its tilde.
        let mut text = String::with_capacity(raw.len());
        let mut escaped = false;
        for c in raw.chars() {
            text.push(if c == '~' && !escaped { ' ' } else { c });
            escaped = c == '\\' && !escaped;
        }
        self.construct(start, Field::Text(text), AtomClass::Ord)
    }

    /// `\mathop{x}` and its kin: the operand reclassed for spacing, a
    /// plain atom as itself, any other group as one atom holding it.
    /// `\mathop` stacks its scripts in display style as an operator does.
    fn classed(&mut self, start: std::ops::Range<usize>, class: AtomClass) -> Atom {
        use crate::token::TokenKind as K;
        let braced = matches!(self.peek().map(|t| &t.kind), Some(K::BeginGroup));
        let mut operand = self.script_operand();
        let end = self.consumed_end(start.end);
        let span = start.start..end;
        // TeX's rule for a braced operand: a group of one Ord atom with no
        // scripts is the atom itself; any other group stays a list, so its
        // scripts stay inside and an operator in it keeps its size. A bare
        // token is the atom itself.
        let plain = match operand.0.as_slice() {
            [Noad::Atom(a)] => {
                !braced || (a.class == AtomClass::Ord && a.sup.is_none() && a.sub.is_none())
            }
            _ => false,
        };
        let mut atom = if plain {
            let Some(Noad::Atom(atom)) = operand.0.pop() else {
                unreachable!("one atom")
            };
            atom
        } else {
            Atom {
                class,
                nucleus: Field::List(operand),
                sup: None,
                sub: None,
                limits: Limits::default(),
                span: span.clone(),
                nucleus_span: span.clone(),
            }
        };
        atom.class = class;
        atom.limits = if class == AtomClass::Op {
            Limits::Default
        } else {
            Limits::NoLimits
        };
        atom.span = span.clone();
        atom.nucleus_span = span;
        atom
    }

    /// `\big(` and its kin, TeX's four fixed sizes of a delimiter: the
    /// word names the size and, by its ending, the class (`l` Open, `r`
    /// Close, `m` Rel, bare Ord); the delimiter follows by the rule of
    /// `\left`, braced or not. A `.` is the invisible box of the size;
    /// anything else leaves the word a literal and parses on.
    fn sized(&mut self, start: std::ops::Range<usize>, name: &str) -> Atom {
        use crate::token::TokenKind as K;
        let (stem, class) = if let Some(stem) = name.strip_suffix('l') {
            (stem, AtomClass::Open)
        } else if let Some(stem) = name.strip_suffix('r') {
            (stem, AtomClass::Close)
        } else if let Some(stem) = name.strip_suffix('m') {
            (stem, AtomClass::Rel)
        } else {
            (name, AtomClass::Ord)
        };
        let size = match stem {
            "big" => 1,
            "Big" => 2,
            "bigg" => 3,
            _ => 4,
        };
        // TeX's \big takes an argument: the delimiter may stand braced
        // on its own, `\big{(}`, read without expanding anything inside.
        if let Some(K::BeginGroup) = self.peek().map(|t| &t.kind) {
            let inner = self
                .tokens
                .get(self.pos + 1)
                .and_then(|t| delimiter_of(&t.kind));
            let closed = matches!(
                self.tokens.get(self.pos + 2).map(|t| &t.kind),
                Some(K::EndGroup)
            );
            if let (Some(ch), true) = (inner, closed) {
                self.pos += 3;
                return self.construct(start, Field::Sized { ch, size }, class);
            }
            return literal_atom(format!("\\{name}"), start);
        }
        match self.delimiter() {
            Some(ch) => self.construct(start, Field::Sized { ch, size }, class),
            None => literal_atom(format!("\\{name}"), start),
        }
    }

    /// `\pmod{n}` and `\mod{n}` as KaTeX's macros expand them, with the
    /// text-style gap in every style (TeX widens it to 18 mu in display):
    /// `\pmod` is 8 mu, `(`, `mod`, 6 mu, the operand, `)`; `\mod` is
    /// 12 mu, `mod`, 6 mu, the operand. One Ord atom holds the list.
    fn modulo(&mut self, start: std::ops::Range<usize>, parens: bool) -> Atom {
        let operand = self.script_operand();
        let end = self.consumed_end(start.end);
        let span = start.start..end;
        let part = |nucleus: Field, class: AtomClass| {
            Noad::Atom(Atom {
                class,
                nucleus,
                sup: None,
                sub: None,
                limits: Limits::NoLimits,
                span: span.clone(),
                nucleus_span: span.clone(),
            })
        };
        let gap = if parens { 8.0 } else { 12.0 } / 18.0;
        let mut items = vec![part(Field::Kern(gap), AtomClass::Ord)];
        if parens {
            items.push(part(Field::Symbol('('), AtomClass::Open));
        }
        items.push(part(Field::Text("mod".into()), AtomClass::Op));
        items.push(part(Field::Kern(6.0 / 18.0), AtomClass::Ord));
        items.extend(operand.0);
        if parens {
            items.push(part(Field::Symbol(')'), AtomClass::Close));
        }
        self.construct(start, Field::List(MathList(items)), AtomClass::Ord)
    }

    /// `\newcommand{\name}[params][default]{body}`: the name braced or
    /// bare, up to nine parameters, one optional default making `#1`
    /// optional at the call. Both spellings define alike and the last
    /// definition wins. A malformed definition answers Err and the
    /// caller degrades what was consumed.
    fn define(&mut self) -> Result<(), ()> {
        use crate::token::TokenKind as K;
        let name = match self.peek().map(|t| &t.kind) {
            Some(K::Command(n)) => {
                let n = n.clone();
                self.pos += 1;
                n
            }
            Some(K::BeginGroup) => {
                self.pos += 1;
                let Some(K::Command(n)) = self.peek().map(|t| &t.kind) else {
                    return Err(());
                };
                let n = n.clone();
                self.pos += 1;
                if !matches!(self.peek().map(|t| &t.kind), Some(K::EndGroup)) {
                    return Err(());
                }
                self.pos += 1;
                n
            }
            _ => return Err(()),
        };
        let mut params = 0usize;
        let mut default = None;
        if matches!(self.peek().map(|t| &t.kind), Some(K::Char('['))) {
            self.pos += 1;
            let Some(K::Char(d @ '0'..='9')) = self.peek().map(|t| &t.kind) else {
                return Err(());
            };
            params = *d as usize - '0' as usize;
            self.pos += 1;
            if !matches!(self.peek().map(|t| &t.kind), Some(K::Char(']'))) {
                return Err(());
            }
            self.pos += 1;
            if matches!(self.peek().map(|t| &t.kind), Some(K::Char('['))) {
                let Some(tokens) = self.bracket_tokens() else {
                    return Err(());
                };
                default = Some(tokens);
            }
        }
        if !matches!(self.peek().map(|t| &t.kind), Some(K::BeginGroup)) {
            return Err(());
        }
        self.pos += 1;
        let (body, closed) = self.balanced_tokens();
        if !closed {
            return Err(());
        }
        self.macros.insert(
            name,
            Macro {
                params,
                default,
                body,
            },
        );
        Ok(())
    }

    /// Expands one macro call at the cursor: arguments read from the
    /// stream, parameters substituted into the body, the result spliced
    /// where the call stood. Body tokens are restamped with the call's
    /// span so their constructs select as the call; argument tokens
    /// keep their own. Answers false on an exhausted budget, the call
    /// degrading to a literal.
    fn expand(&mut self, name: &str, start: std::ops::Range<usize>) -> bool {
        use crate::token::TokenKind as K;
        if self.expansions == 0 || self.spliced >= SPLICE_CEILING {
            return false;
        }
        self.expansions -= 1;
        let Some(mac) = self.macros.get(name).cloned() else {
            return false;
        };
        let mut args: Vec<Vec<crate::token::Token>> = Vec::new();
        if let Some(default) = mac.default {
            if matches!(self.peek().map(|t| &t.kind), Some(K::Char('['))) {
                args.push(self.bracket_tokens().unwrap_or_default());
            } else {
                args.push(default);
            }
        }
        while args.len() < mac.params {
            args.push(self.macro_arg());
        }
        let span = start.start..self.consumed_end(start.end);
        let mut out: Vec<crate::token::Token> = Vec::new();
        let mut i = 0;
        while i < mac.body.len() {
            if matches!(mac.body[i].kind, K::Char('#')) && i + 1 < mac.body.len() {
                if let K::Char(d) = mac.body[i + 1].kind {
                    if d == '#' {
                        out.push(crate::token::Token {
                            kind: K::Char('#'),
                            span: span.clone(),
                            origin: None,
                        });
                        i += 2;
                        continue;
                    }
                    if let Some(n) = d.to_digit(10) {
                        // #1 through #9; #0 and a count past the
                        // arguments splice nothing.
                        if let Some(k) = (n as usize).checked_sub(1) {
                            if k < args.len() {
                                out.extend(args[k].iter().cloned());
                            }
                        }
                        i += 2;
                        continue;
                    }
                }
            }
            out.push(crate::token::Token {
                kind: mac.body[i].kind.clone(),
                span: span.clone(),
                origin: mac.body[i]
                    .origin
                    .clone()
                    .or_else(|| Some(mac.body[i].span.clone())),
            });
            i += 1;
        }
        self.spliced += out.len();
        self.tokens.splice(self.pos..self.pos, out);
        true
    }

    /// One macro argument: a braced group's tokens, or the single next
    /// token. End of input answers empty.
    fn macro_arg(&mut self) -> Vec<crate::token::Token> {
        use crate::token::TokenKind as K;
        match self.peek().map(|t| &t.kind) {
            Some(K::BeginGroup) => {
                self.pos += 1;
                self.balanced_tokens().0
            }
            Some(_) => self.next().into_iter().collect(),
            None => Vec::new(),
        }
    }

    /// Captures tokens to the matching group closer, the opener already
    /// consumed and the closer consumed but excluded. Answers whether
    /// the closer was found.
    fn balanced_tokens(&mut self) -> (Vec<crate::token::Token>, bool) {
        use crate::token::TokenKind as K;
        let mut depth = 1usize;
        let mut out = Vec::new();
        while let Some(tok) = self.next() {
            match tok.kind {
                K::BeginGroup => depth += 1,
                K::EndGroup => {
                    depth -= 1;
                    if depth == 0 {
                        return (out, true);
                    }
                }
                _ => {}
            }
            out.push(tok);
        }
        (out, false)
    }

    /// Captures a bracketed `[...]` token run at the cursor, braces
    /// hiding any `]` inside them. A missing closer restores the cursor
    /// and answers none.
    fn bracket_tokens(&mut self) -> Option<Vec<crate::token::Token>> {
        use crate::token::TokenKind as K;
        let saved = self.pos;
        self.pos += 1;
        let mut depth = 0usize;
        let mut out = Vec::new();
        while let Some(tok) = self.next() {
            match &tok.kind {
                K::BeginGroup => depth += 1,
                K::EndGroup => depth = depth.saturating_sub(1),
                K::Char(']') if depth == 0 => return Some(out),
                _ => {}
            }
            out.push(tok);
        }
        self.pos = saved;
        None
    }

    /// `\begin{name} ... \end{name}`: cells split on `&`, rows on `\\`,
    /// each environment bringing its alignment, gap rule and fences. An
    /// unknown name skips to its `\end` and degrades to a literal; an
    /// unterminated body degrades whole.
    fn environment(&mut self, start: std::ops::Range<usize>) -> Atom {
        let Some(name) = self.env_name() else {
            let end = self.consumed_end(start.end);
            let span = start.start..end;
            return literal_atom(
                self.src.get(span.clone()).unwrap_or("\\begin").to_string(),
                span,
            );
        };
        type Fences = Option<(char, Option<char>)>;
        let known: Option<(Vec<ColAlign>, TableGaps, bool, Fences)> = match name.as_str() {
            "matrix" => Some((vec![ColAlign::Center], TableGaps::Em(1.0), false, None)),
            "smallmatrix" => Some((vec![ColAlign::Center], TableGaps::Em(0.5), true, None)),
            "pmatrix" => Some((
                vec![ColAlign::Center],
                TableGaps::Em(1.0),
                false,
                Some(('(', Some(')'))),
            )),
            "bmatrix" => Some((
                vec![ColAlign::Center],
                TableGaps::Em(1.0),
                false,
                Some(('[', Some(']'))),
            )),
            "Bmatrix" => Some((
                vec![ColAlign::Center],
                TableGaps::Em(1.0),
                false,
                Some(('{', Some('}'))),
            )),
            "vmatrix" => Some((
                vec![ColAlign::Center],
                TableGaps::Em(1.0),
                false,
                Some(('|', Some('|'))),
            )),
            "Vmatrix" => Some((
                vec![ColAlign::Center],
                TableGaps::Em(1.0),
                false,
                Some(('\u{2016}', Some('\u{2016}'))),
            )),
            "cases" => Some((
                vec![ColAlign::Left],
                TableGaps::Em(1.0),
                false,
                Some(('{', None)),
            )),
            "aligned" => Some((
                vec![ColAlign::Right, ColAlign::Left],
                TableGaps::Pairs,
                false,
                None,
            )),
            "array" => Some((self.array_spec(), TableGaps::Em(1.0), false, None)),
            _ => None,
        };
        let Some((align, gaps, small, fences)) = known else {
            self.skip_environment();
            let end = self.consumed_end(start.end);
            let span = start.start..end;
            return literal_atom(
                self.src.get(span.clone()).unwrap_or("\\begin").to_string(),
                span,
            );
        };
        let (rows, terminated) = self.table_cells();
        let end = self.consumed_end(start.end);
        let span = start.start..end;
        if !terminated {
            return literal_atom(
                self.src.get(span.clone()).unwrap_or("\\begin").to_string(),
                span,
            );
        }
        let table = Atom {
            class: AtomClass::Ord,
            nucleus: Field::Table {
                rows,
                align,
                gaps,
                small,
            },
            sup: None,
            sub: None,
            limits: Limits::default(),
            span: span.clone(),
            nucleus_span: span.clone(),
        };
        match fences {
            Some((open, close)) => Atom {
                class: AtomClass::Inner,
                nucleus: Field::LeftRight {
                    open: Some(open),
                    close,
                    body: MathList(vec![Noad::Atom(table)]),
                },
                sup: None,
                sub: None,
                limits: Limits::default(),
                span: span.clone(),
                nucleus_span: span,
            },
            None => table,
        }
    }

    /// The braced environment name after `\begin` or `\end`: letters
    /// and stars only, anything else answers none.
    fn env_name(&mut self) -> Option<String> {
        use crate::token::TokenKind as K;
        if !matches!(self.peek().map(|t| &t.kind), Some(K::BeginGroup)) {
            return None;
        }
        self.pos += 1;
        let mut name = String::new();
        while let Some(tok) = self.peek() {
            match &tok.kind {
                K::Char(c) => {
                    name.push(*c);
                    self.pos += 1;
                }
                K::EndGroup => {
                    self.pos += 1;
                    return Some(name);
                }
                _ => return None,
            }
        }
        None
    }

    /// `array`'s column specification: `r`, `c`, `l` collect, rules and
    /// separators pass quietly, a missing group means one centered
    /// column.
    fn array_spec(&mut self) -> Vec<ColAlign> {
        use crate::token::TokenKind as K;
        let mut align = Vec::new();
        if matches!(self.peek().map(|t| &t.kind), Some(K::BeginGroup)) {
            self.pos += 1;
            while let Some(tok) = self.peek() {
                match &tok.kind {
                    K::Char('l') => align.push(ColAlign::Left),
                    K::Char('c') => align.push(ColAlign::Center),
                    K::Char('r') => align.push(ColAlign::Right),
                    K::Char(_) => {}
                    K::EndGroup => {
                        self.pos += 1;
                        break;
                    }
                    _ => break,
                }
                self.pos += 1;
            }
        }
        if align.is_empty() {
            align.push(ColAlign::Center);
        }
        align
    }

    /// Consumes an unknown environment through its matching `\end`,
    /// nested environments counted; end of input stops the skip.
    fn skip_environment(&mut self) {
        use crate::token::TokenKind as K;
        let mut depth = 1usize;
        while let Some(tok) = self.next() {
            match &tok.kind {
                K::Command(c) if c == "begin" => depth += 1,
                K::Command(c) if c == "end" => {
                    let _ = self.env_name();
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    /// An environment body: atoms accumulate into cells, `&` closes a
    /// cell, `\\` a row, `\end` the table. End of input or the enclosing
    /// group's closer answers unterminated, the closer left in place.
    fn table_cells(&mut self) -> (Vec<Vec<MathList>>, bool) {
        use crate::token::TokenKind as K;
        let mut rows: Vec<Vec<MathList>> = Vec::new();
        let mut row: Vec<MathList> = Vec::new();
        let mut cell = Group::default();
        loop {
            let Some(tok) = self.peek() else {
                return (rows, false);
            };
            match &tok.kind {
                K::Align => {
                    self.pos += 1;
                    row.push(std::mem::take(&mut cell).finish());
                }
                K::Command(name) if name == "\\" => {
                    self.pos += 1;
                    row.push(std::mem::take(&mut cell).finish());
                    rows.push(std::mem::take(&mut row));
                }
                K::Command(name) if name == "end" => {
                    self.pos += 1;
                    let _ = self.env_name();
                    row.push(std::mem::take(&mut cell).finish());
                    // A trailing \\ leaves one empty row; TeX drops it.
                    let trailing_empty = row.len() == 1 && row[0].0.is_empty() && !rows.is_empty();
                    if !trailing_empty {
                        rows.push(std::mem::take(&mut row));
                    }
                    return (rows, true);
                }
                K::EndGroup => {
                    return (rows, false);
                }
                _ => {
                    if !self.read_item(&mut cell) {
                        return (rows, false);
                    }
                }
            }
        }
    }

    /// `\sqrt{x}` with the optional `[degree]`.
    fn radical(&mut self, start: std::ops::Range<usize>) -> Atom {
        use crate::token::TokenKind as K;
        let degree = if matches!(self.peek().map(|t| &t.kind), Some(K::Char('['))) {
            // LaTeX reads an optional argument by its tokens, up to the `]`
            // at brace depth zero, before anything in it expands: a macro
            // inside the degree cannot swallow the bracket. Without a
            // closing bracket the degree runs to the end, as before.
            match self.bracket_tokens() {
                Some(tokens) => Some(self.sub_list(tokens)),
                None => {
                    self.pos += 1;
                    Some(self.list_until_char(']'))
                }
            }
        } else {
            None
        };
        let radicand = self.script_operand();
        self.construct(start, Field::Radical { radicand, degree }, AtomClass::Ord)
    }

    /// A list parsed from tokens already cut out of the input, an optional
    /// argument, with this parser's macros and budgets, which it hands
    /// back.
    fn sub_list(&mut self, tokens: Vec<crate::token::Token>) -> MathList {
        let mut sub = Parser {
            tokens,
            pos: 0,
            src: self.src,
            macros: std::mem::take(&mut self.macros),
            expansions: self.expansions,
            spliced: self.spliced,
        };
        let list = sub.list(true);
        self.macros = sub.macros;
        self.expansions = sub.expansions;
        self.spliced = sub.spliced;
        list
    }

    /// Elements up to a closing character, consumed; end of input closes.
    fn list_until_char(&mut self, closer: char) -> MathList {
        use crate::token::TokenKind as K;
        let mut group = Group::default();
        while let Some(tok) = self.peek() {
            if matches!(&tok.kind, K::Char(c) if *c == closer) {
                self.pos += 1;
                break;
            }
            if matches!(tok.kind, K::EndGroup) {
                break;
            }
            if !self.read_item(&mut group) {
                break;
            }
        }
        group.finish()
    }

    /// `\left⟨delim⟩ ... \right⟨delim⟩`. A missing `\right` fails open at
    /// the end of input or at the enclosing group's closer, which stays
    /// for the group to consume.
    fn left_right(&mut self, start: std::ops::Range<usize>) -> Atom {
        use crate::token::TokenKind as K;
        let open = self.delimiter().flatten();
        let mut group = Group::default();
        let mut close = None;
        while let Some(tok) = self.peek() {
            match &tok.kind {
                K::Command(name) if name == "right" => {
                    self.pos += 1;
                    close = self.delimiter().flatten();
                    break;
                }
                K::EndGroup => break,
                _ => {
                    if !self.read_item(&mut group) {
                        break;
                    }
                }
            }
        }
        self.construct(
            start,
            Field::LeftRight {
                open,
                close,
                body: group.finish(),
            },
            AtomClass::Inner,
        )
    }

    /// One delimiter token after `\left`, `\right` or a `\big` word, as
    /// `delimiter_of` reads it, a user macro expanded first. Anything
    /// else answers `None` and stays.
    fn delimiter(&mut self) -> Option<Option<char>> {
        use crate::token::TokenKind as K;
        // A user macro expands first, as everywhere else.
        if let Some(K::Command(name)) = self.peek().map(|t| &t.kind) {
            if self.macros.contains_key(name) {
                let name = name.clone();
                let span = self.peek().map(|t| t.span.clone()).unwrap_or_default();
                self.pos += 1;
                if self.expand(&name, span) {
                    return self.delimiter();
                }
                self.pos -= 1;
                return None;
            }
        }
        let resolved = self.peek().and_then(|t| delimiter_of(&t.kind));
        if resolved.is_some() {
            self.pos += 1;
        }
        resolved
    }

    /// Attaches every following script marker to the atom. Repeated markers
    /// merge into the existing script list, TeX's double-script degraded
    /// quietly instead of erroring.
    fn scripts(&mut self, atom: &mut Atom) {
        use crate::token::TokenKind as K;
        while let Some(tok) = self.peek() {
            let span = tok.span.clone();
            match tok.kind {
                K::Command(ref name) if name == "limits" || name == "nolimits" => {
                    // TeX's postfix limit modifiers bind to an operator;
                    // on anything else they fall through as literals.
                    if atom.class != AtomClass::Op {
                        break;
                    }
                    atom.limits = if name == "limits" {
                        Limits::Limits
                    } else {
                        Limits::NoLimits
                    };
                    atom.span.end = atom.span.end.max(span.end);
                    self.pos += 1;
                }
                K::Prime => {
                    self.pos += 1;
                    let prime = Atom {
                        class: AtomClass::Ord,
                        nucleus: Field::Symbol('\u{2032}'),
                        sup: None,
                        sub: None,
                        limits: Limits::default(),
                        span: span.clone(),
                        nucleus_span: span.clone(),
                    };
                    atom.sup
                        .get_or_insert_with(MathList::default)
                        .0
                        .push(Noad::Atom(prime));
                    atom.span.end = atom.span.end.max(span.end);
                }
                K::Sup => {
                    self.pos += 1;
                    let operand = self.script_operand();
                    let target = atom.sup.get_or_insert_with(MathList::default);
                    target.0.extend(operand.0);
                    atom.span.end = self.consumed_end(atom.span.end);
                }
                K::Sub => {
                    self.pos += 1;
                    let operand = self.script_operand();
                    let target = atom.sub.get_or_insert_with(MathList::default);
                    target.0.extend(operand.0);
                    atom.span.end = self.consumed_end(atom.span.end);
                }
                _ => break,
            }
        }
    }

    fn consumed_end(&self, fallback: usize) -> usize {
        self.tokens
            .get(self.pos.saturating_sub(1))
            .map(|t| t.span.end)
            .unwrap_or(fallback)
    }

    /// One script operand: a single token or a braced group. A missing or
    /// impossible operand yields an empty list.
    fn script_operand(&mut self) -> MathList {
        use crate::token::TokenKind as K;
        let Some(tok) = self.peek() else {
            return MathList::default();
        };
        let span = tok.span.clone();
        match &tok.kind {
            K::BeginGroup => {
                self.pos += 1;
                self.list(false)
            }
            // A macro expands first, and the operand is what follows it.
            K::Command(name) if self.macros.contains_key(name.as_str()) => {
                let name = name.clone();
                self.pos += 1;
                if self.expand(&name, span) {
                    return self.script_operand();
                }
                self.pos -= 1;
                match self.atom() {
                    Some(atom) => MathList(vec![Noad::Atom(atom)]),
                    None => MathList::default(),
                }
            }
            // A closer of the structure around (a row break, `\\right`,
            // `\\end`), a script marker or a word that acts on the group
            // (`\\over`, a switch) is no operand: the operand is empty and
            // the token stays for its reader, TeX's missing argument.
            K::Command(name)
                if name == "\\"
                    || name == "right"
                    || name == "end"
                    || Marker::of(name).is_some() =>
            {
                MathList::default()
            }
            K::Sup | K::Sub | K::Prime | K::Align | K::EndGroup => MathList::default(),
            K::Char(_) | K::Command(_) => match self.atom() {
                Some(atom) => MathList(vec![Noad::Atom(atom)]),
                None => MathList::default(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atoms(tex: &str) -> Vec<Atom> {
        parse(tex).atoms().cloned().collect()
    }

    #[test]
    fn the_tables_are_sorted_and_duplicate_free() {
        for pair in VOCABULARY.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
        for pair in OP_NAMES.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
        for pair in DELIMITERS.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
        for pair in ACCENTS.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
    }

    #[test]
    fn commands_resolve_symbol_and_class() {
        // The binary operator between two operands, where TeX keeps it one.
        let a = atoms("\\alpha\\pm\\beta\\leq\\sum");
        assert_eq!(a.len(), 5);
        assert_eq!(a[0].nucleus, Field::Symbol('\u{03B1}'));
        assert_eq!(a[0].class, AtomClass::Ord);
        assert_eq!(a[1].nucleus, Field::Symbol('\u{00B1}'));
        assert_eq!(a[1].class, AtomClass::Bin);
        assert_eq!(a[3].class, AtomClass::Rel);
        assert_eq!(a[4].nucleus, Field::Symbol('\u{2211}'));
        assert_eq!(a[4].class, AtomClass::Op);
    }

    #[test]
    fn plain_characters_classify() {
        let a = atoms("x+2=y,");
        let classes: Vec<AtomClass> = a.iter().map(|a| a.class).collect();
        assert_eq!(
            classes,
            vec![
                AtomClass::Ord,
                AtomClass::Bin,
                AtomClass::Ord,
                AtomClass::Rel,
                AtomClass::Ord,
                AtomClass::Punct,
            ]
        );
    }

    #[test]
    fn scripts_attach_to_their_atom() {
        let a = atoms("x^2");
        assert_eq!(a.len(), 1);
        let sup = a[0].sup.as_ref().expect("sup");
        assert_eq!(sup.atoms().next().unwrap().nucleus, Field::Symbol('2'));
        assert!(a[0].sub.is_none());

        let a = atoms("a_i");
        assert!(a[0].sup.is_none());
        assert!(a[0].sub.is_some());

        let a = atoms("x_i^2");
        assert!(a[0].sup.is_some() && a[0].sub.is_some());
    }

    #[test]
    fn braced_scripts_and_group_nuclei() {
        let a = atoms("x^{ab}");
        let sup = a[0].sup.as_ref().unwrap();
        assert_eq!(sup.atoms().count(), 2);

        let a = atoms("{ab}c");
        assert_eq!(a.len(), 2);
        match &a[0].nucleus {
            Field::List(inner) => assert_eq!(inner.atoms().count(), 2),
            other => panic!("expected group nucleus, got {other:?}"),
        }
    }

    #[test]
    fn hyphen_reads_as_minus() {
        let a = atoms("a-b");
        assert_eq!(a[1].nucleus, Field::Symbol('\u{2212}'));
        assert_eq!(a[1].class, AtomClass::Bin);
    }

    #[test]
    fn binary_atoms_demote_where_no_operand_precedes() {
        // Leading, after another Bin, after Rel, after Open: Ord.
        let a = atoms("+x");
        assert_eq!(a[0].class, AtomClass::Ord);
        let a = atoms("a+-b");
        assert_eq!(a[2].class, AtomClass::Ord);
        let a = atoms("a=-b");
        assert_eq!(a[2].class, AtomClass::Ord);
        let a = atoms("(-b)");
        assert_eq!(a[1].class, AtomClass::Ord);
        // With a real left operand it stays binary.
        let a = atoms("a-b");
        assert_eq!(a[1].class, AtomClass::Bin);
    }

    #[test]
    fn unknown_commands_become_literals() {
        let a = atoms("\\foobar x");
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].nucleus, Field::Literal("\\foobar".into()));
        assert_eq!(a[0].class, AtomClass::Ord);
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
    }

    #[test]
    fn escaped_percent_is_a_sign_and_a_bare_one_a_comment() {
        let a = atoms("15\\% % off\nx");
        let symbols: Vec<Field> = a.iter().map(|a| a.nucleus.clone()).collect();
        assert_eq!(
            symbols,
            vec![
                Field::Symbol('1'),
                Field::Symbol('5'),
                Field::Symbol('%'),
                Field::Symbol('x'),
            ]
        );
        assert_eq!(a[2].class, AtomClass::Ord);
    }

    #[test]
    fn escaped_specials_become_their_character() {
        let a = atoms("\\#\\&\\$\\_");
        assert_eq!(a.len(), 4);
        for (atom, ch) in a.iter().zip(['#', '&', '$', '_']) {
            assert_eq!(atom.nucleus, Field::Symbol(ch), "\\{ch}");
            assert_eq!(atom.class, AtomClass::Ord, "\\{ch}");
        }
    }

    #[test]
    fn stray_closers_and_alignment_degrade_to_literals() {
        let a = atoms("}x");
        assert_eq!(a[0].nucleus, Field::Literal("}".into()));
        let a = atoms("a&b");
        assert_eq!(a[1].nucleus, Field::Literal("&".into()));
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn primes_become_superscripts() {
        let a = atoms("x'");
        let sup = a[0].sup.as_ref().expect("prime lands in sup");
        assert_eq!(
            sup.atoms().next().unwrap().nucleus,
            Field::Symbol('\u{2032}')
        );
        let a = atoms("x''");
        assert_eq!(a[0].sup.as_ref().unwrap().atoms().count(), 2);
    }

    #[test]
    fn spans_stamp_source_bytes() {
        let a = atoms("x^2+\\alpha");
        assert_eq!(a[0].span, 0..3);
        assert_eq!(a[1].span, 3..4);
        assert_eq!(a[2].span, 4..10);
    }

    #[test]
    fn hostile_input_never_panics() {
        for tex in [
            "", "^", "_", "^^", "{", "}", "{{{", "}}}", "x^", "x_", "\\", "x\\", "&", "\\\\",
            "a^{b", "π^é", "%", "x%",
        ] {
            let _ = parse(tex);
        }
    }

    #[test]
    fn unclosed_group_closes_at_end() {
        let a = atoms("{ab");
        assert_eq!(a.len(), 1);
        match &a[0].nucleus {
            Field::List(inner) => assert_eq!(inner.atoms().count(), 2),
            other => panic!("expected group, got {other:?}"),
        }
    }

    #[test]
    fn frac_takes_two_arguments() {
        let a = atoms("\\frac{a+b}{2}x");
        assert_eq!(a.len(), 2);
        let Field::Fraction {
            numerator,
            denominator,
            bar,
            ..
        } = &a[0].nucleus
        else {
            panic!("expected fraction, got {:?}", a[0].nucleus)
        };
        assert!(bar);
        assert_eq!(numerator.atoms().count(), 3);
        assert_eq!(denominator.atoms().count(), 1);
        assert_eq!(a[0].class, AtomClass::Inner);
        // Single-token arguments work without braces.
        let a = atoms("\\frac12");
        let Field::Fraction { numerator, .. } = &a[0].nucleus else {
            panic!()
        };
        assert_eq!(
            numerator.atoms().next().unwrap().nucleus,
            Field::Symbol('1')
        );
    }

    #[test]
    fn binom_is_a_barless_stack_in_parens() {
        let a = atoms("\\binom{n}{k}");
        let Field::LeftRight { open, close, body } = &a[0].nucleus else {
            panic!("expected delimited group, got {:?}", a[0].nucleus)
        };
        assert_eq!((*open, *close), (Some('('), Some(')')));
        let inner = body.atoms().next().expect("stack inside");
        let Field::Fraction { bar, .. } = &inner.nucleus else {
            panic!("expected stack, got {:?}", inner.nucleus)
        };
        assert!(!bar);
    }

    #[test]
    fn sqrt_takes_optional_degree() {
        let a = atoms("\\sqrt{x+1}");
        let Field::Radical { radicand, degree } = &a[0].nucleus else {
            panic!("expected radical, got {:?}", a[0].nucleus)
        };
        assert_eq!(radicand.atoms().count(), 3);
        assert!(degree.is_none());
        let a = atoms("\\sqrt[3]{x}");
        let Field::Radical { degree, .. } = &a[0].nucleus else {
            panic!()
        };
        let deg = degree.as_ref().expect("degree parsed");
        assert_eq!(deg.atoms().next().unwrap().nucleus, Field::Symbol('3'));
    }

    #[test]
    fn left_right_wraps_its_body() {
        let a = atoms("\\left( \\frac{a}{b} \\right)^2");
        assert_eq!(a.len(), 1);
        let Field::LeftRight { open, close, body } = &a[0].nucleus else {
            panic!("expected delimited group, got {:?}", a[0].nucleus)
        };
        assert_eq!((*open, *close), (Some('('), Some(')')));
        assert_eq!(body.atoms().count(), 1);
        assert_eq!(a[0].class, AtomClass::Inner);
        assert!(a[0].sup.is_some(), "the script rides the whole group");
        // The dot delimiter means none; command delimiters resolve.
        let a = atoms("\\left. x \\right\\}");
        let Field::LeftRight { open, close, .. } = &a[0].nucleus else {
            panic!()
        };
        assert_eq!((*open, *close), (None, Some('}')));
    }

    #[test]
    fn unmatched_left_fails_open() {
        let a = atoms("\\left( x");
        assert!(!a.is_empty());
        let flat = parse("\\left( x");
        assert!(flat.atoms().count() >= 1, "never panics, keeps content");
        let _ = parse("x \\right)");
        let _ = parse("\\left");
        let _ = parse("\\left(\\left[x");
    }

    #[test]
    fn limits_modifiers_bind_to_operators() {
        let a = atoms("\\sum\\limits x");
        assert_eq!(a[0].limits, Limits::Limits);
        assert_eq!(a.len(), 2);
        let a = atoms("\\int\\nolimits x");
        assert_eq!(a[0].limits, Limits::NoLimits);
        // On a non-operator the modifier is a quiet literal.
        let a = atoms("x\\limits");
        assert_eq!(a[1].nucleus, Field::Literal("\\limits".into()));
    }

    #[test]
    fn spacing_commands_become_kerns() {
        let a = atoms("a\\,b");
        assert_eq!(a.len(), 3);
        assert_eq!(a[1].nucleus, Field::Kern(3.0 / 18.0));
        let a = atoms("\\:");
        assert_eq!(a[0].nucleus, Field::Kern(4.0 / 18.0));
        let a = atoms("\\;");
        assert_eq!(a[0].nucleus, Field::Kern(5.0 / 18.0));
        let a = atoms("\\!");
        assert_eq!(a[0].nucleus, Field::Kern(-3.0 / 18.0));
        let a = atoms("\\quad");
        assert_eq!(a[0].nucleus, Field::Kern(1.0));
        let a = atoms("\\qquad");
        assert_eq!(a[0].nucleus, Field::Kern(2.0));
    }

    #[test]
    fn kerns_are_transparent_to_demotion() {
        // The kern hides nothing: + still has its left operand.
        let a = atoms("a\\,+b");
        assert_eq!(a[2].class, AtomClass::Bin);
        // A leading kern provides no operand.
        let a = atoms("\\,+b");
        assert_eq!(a[1].class, AtomClass::Ord);
    }

    #[test]
    fn alphabet_commands_remap_codepoints() {
        for (tex, mapped) in [
            ("\\mathbb{R}", '\u{211D}'),
            ("\\mathbb{A}", '\u{1D538}'),
            ("\\mathbf{A}", '\u{1D400}'),
            ("\\mathit{A}", '\u{1D434}'),
            ("\\mathcal{L}", '\u{2112}'),
            ("\\mathfrak{g}", '\u{1D524}'),
            ("\\mathsf{x}", '\u{1D5D1}'),
            ("\\mathtt{0}", '\u{1D7F6}'),
        ] {
            let a = atoms(tex);
            assert_eq!(a[0].nucleus, Field::Symbol(mapped), "{tex}");
        }
    }

    #[test]
    fn alphabet_commands_reach_nested_groups() {
        let a = atoms("\\mathbf{ab}");
        let Field::List(inner) = &a[0].nucleus else {
            panic!("expected group nucleus, got {:?}", a[0].nucleus)
        };
        let mapped: Vec<Field> = inner.atoms().map(|at| at.nucleus.clone()).collect();
        assert_eq!(
            mapped,
            vec![Field::Symbol('\u{1D41A}'), Field::Symbol('\u{1D41B}')]
        );
    }

    #[test]
    fn text_keeps_its_source_verbatim() {
        let a = atoms("\\text{if }x");
        assert_eq!(a[0].nucleus, Field::Text("if ".into()));
        assert_eq!(a[0].class, AtomClass::Ord);
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
        // Nested braces stay inside.
        let a = atoms("\\text{a{b}c}");
        assert_eq!(a[0].nucleus, Field::Text("a{b}c".into()));
        // With no brace the argument is the next token.
        let a = atoms("\\text x");
        assert_eq!(a[0].nucleus, Field::Text("x".into()));
    }

    #[test]
    fn operator_names_are_upright_op_atoms() {
        let a = atoms("\\sin x");
        assert_eq!(a[0].nucleus, Field::Text("sin".into()));
        assert_eq!(a[0].class, AtomClass::Op);
        assert_eq!(a[0].limits, Limits::NoLimits);
        let a = atoms("\\lim_n x");
        assert_eq!(a[0].nucleus, Field::Text("lim".into()));
        assert_eq!(
            a[0].limits,
            Limits::Default,
            "lim stacks its limits in display"
        );
    }

    #[test]
    fn accents_parse_with_their_stretch_flag() {
        let a = atoms("\\hat x");
        let Field::Accent {
            accent,
            stretch,
            base,
        } = &a[0].nucleus
        else {
            panic!("expected accent, got {:?}", a[0].nucleus)
        };
        assert_eq!((*accent, *stretch), ('\u{0302}', false));
        assert_eq!(base.atoms().next().unwrap().nucleus, Field::Symbol('x'));
        let a = atoms("\\widehat{abc}");
        let Field::Accent { stretch, base, .. } = &a[0].nucleus else {
            panic!("expected accent, got {:?}", a[0].nucleus)
        };
        assert!(stretch);
        assert_eq!(base.atoms().count(), 3);
        let a = atoms("\\vec v");
        let Field::Accent { accent, .. } = &a[0].nucleus else {
            panic!("expected accent, got {:?}", a[0].nucleus)
        };
        assert_eq!(*accent, '\u{20D7}');
        let a = atoms("\\bar y");
        let Field::Accent { accent, .. } = &a[0].nucleus else {
            panic!("expected accent, got {:?}", a[0].nucleus)
        };
        assert_eq!(*accent, '\u{0304}');
    }

    #[test]
    fn environments_parse_to_tables() {
        let a = atoms("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}");
        assert_eq!(a.len(), 1);
        let Field::LeftRight { open, close, body } = &a[0].nucleus else {
            panic!("expected fenced table, got {:?}", a[0].nucleus)
        };
        assert_eq!((*open, *close), (Some('('), Some(')')));
        let inner = body.atoms().next().expect("the table inside");
        let Field::Table {
            rows, align, small, ..
        } = &inner.nucleus
        else {
            panic!("expected table, got {:?}", inner.nucleus)
        };
        assert!(!small);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 2);
        assert_eq!(
            rows[1][1].atoms().next().unwrap().nucleus,
            Field::Symbol('d')
        );
        assert_eq!(align, &vec![ColAlign::Center]);
        // A bare matrix takes no fences; smallmatrix flags its size.
        let a = atoms("\\begin{matrix} a \\end{matrix}");
        assert!(matches!(&a[0].nucleus, Field::Table { small: false, .. }));
        let a = atoms("\\begin{smallmatrix} a \\end{smallmatrix}");
        assert!(matches!(&a[0].nucleus, Field::Table { small: true, .. }));
    }

    #[test]
    fn the_matrix_family_picks_its_fences() {
        for (tex, open, close) in [
            ("\\begin{bmatrix} a \\end{bmatrix}", '[', ']'),
            ("\\begin{vmatrix} a \\end{vmatrix}", '|', '|'),
            ("\\begin{Vmatrix} a \\end{Vmatrix}", '\u{2016}', '\u{2016}'),
        ] {
            let a = atoms(tex);
            let Field::LeftRight {
                open: o, close: c, ..
            } = &a[0].nucleus
            else {
                panic!("expected fenced table for {tex}, got {:?}", a[0].nucleus)
            };
            assert_eq!((*o, *c), (Some(open), Some(close)), "{tex}");
        }
    }

    #[test]
    fn cases_aligned_and_array_set_their_columns() {
        let a = atoms("\\begin{cases} x & y \\\\ 0 & z \\end{cases}");
        let Field::LeftRight { open, close, body } = &a[0].nucleus else {
            panic!("expected braced table, got {:?}", a[0].nucleus)
        };
        assert_eq!((*open, *close), (Some('{'), None));
        let inner = body.atoms().next().unwrap();
        let Field::Table { align, .. } = &inner.nucleus else {
            panic!("expected table, got {:?}", inner.nucleus)
        };
        assert_eq!(align, &vec![ColAlign::Left]);

        let a = atoms("\\begin{aligned} x &= y \\\\ z &= w \\end{aligned}");
        let Field::Table { align, rows, .. } = &a[0].nucleus else {
            panic!("expected table, got {:?}", a[0].nucleus)
        };
        assert_eq!(align, &vec![ColAlign::Right, ColAlign::Left]);
        assert_eq!(rows.len(), 2);

        let a = atoms("\\begin{array}{rcl} a & b & c \\end{array}");
        let Field::Table { align, .. } = &a[0].nucleus else {
            panic!("expected table, got {:?}", a[0].nucleus)
        };
        assert_eq!(
            align,
            &vec![ColAlign::Right, ColAlign::Center, ColAlign::Left]
        );
    }

    #[test]
    fn all_command_tables_are_sorted_and_duplicate_free() {
        for pair in DELIMITERS.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
        for pair in ACCENTS.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
        for pair in OP_NAMES.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
    }

    #[test]
    fn the_greek_alphabet_is_complete() {
        // Every Greek letter command resolves to a symbol, never a
        // literal: a row lost in a table rewrite fails here by name.
        for name in [
            "alpha",
            "beta",
            "gamma",
            "delta",
            "epsilon",
            "zeta",
            "eta",
            "theta",
            "iota",
            "kappa",
            "lambda",
            "mu",
            "nu",
            "xi",
            "omicron",
            "pi",
            "rho",
            "sigma",
            "tau",
            "upsilon",
            "phi",
            "chi",
            "psi",
            "omega",
            "varepsilon",
            "vartheta",
            "varkappa",
            "varpi",
            "varrho",
            "varsigma",
            "varphi",
            "digamma",
            "Alpha",
            "Beta",
            "Gamma",
            "Delta",
            "Epsilon",
            "Zeta",
            "Eta",
            "Theta",
            "Iota",
            "Kappa",
            "Lambda",
            "Mu",
            "Nu",
            "Xi",
            "Omicron",
            "Pi",
            "Rho",
            "Sigma",
            "Tau",
            "Upsilon",
            "Phi",
            "Chi",
            "Psi",
            "Omega",
        ] {
            let a = atoms(&format!("\\{name}"));
            assert!(
                matches!(a[0].nucleus, Field::Symbol(_)),
                "\\{name} must resolve, got {:?}",
                a[0].nucleus
            );
        }
    }

    #[test]
    fn the_sweep_samples_resolve() {
        use AtomClass as C;
        for (tex, ch, class) in [
            // Greek completions and variants.
            ("\\Upsilon", '\u{03A5}', C::Ord),
            ("\\varpi", '\u{03D6}', C::Ord),
            ("\\digamma", '\u{03DD}', C::Ord),
            // Binary operators.
            ("\\ast", '\u{2217}', C::Bin),
            ("\\circ", '\u{2218}', C::Bin),
            ("\\ltimes", '\u{22C9}', C::Bin),
            ("\\boxplus", '\u{229E}', C::Bin),
            // Relations.
            ("\\ll", '\u{226A}', C::Rel),
            ("\\preceq", '\u{2AAF}', C::Rel),
            ("\\vdash", '\u{22A2}', C::Rel),
            ("\\rightleftharpoons", '\u{21CC}', C::Rel),
            // Negations.
            ("\\nleq", '\u{2270}', C::Rel),
            ("\\nvdash", '\u{22AC}', C::Rel),
            ("\\nsubseteq", '\u{2288}', C::Rel),
            // Arrows.
            ("\\hookrightarrow", '\u{21AA}', C::Rel),
            ("\\Longrightarrow", '\u{27F9}', C::Rel),
            ("\\iff", '\u{27FA}', C::Rel),
            ("\\twoheadrightarrow", '\u{21A0}', C::Rel),
            // Big operators.
            ("\\bigcup", '\u{22C3}', C::Op),
            ("\\oint", '\u{222E}', C::Op),
            ("\\bigoplus", '\u{2A01}', C::Op),
            // Miscellany.
            ("\\aleph", '\u{2135}', C::Ord),
            ("\\forall", '\u{2200}', C::Ord),
            ("\\hbar", '\u{210F}', C::Ord),
            ("\\S", '\u{00A7}', C::Ord),
            ("\\vdots", '\u{22EE}', C::Ord),
            // Aliases.
            ("\\le", '\u{2264}', C::Rel),
            ("\\land", '\u{2227}', C::Bin),
            ("\\gets", '\u{2190}', C::Rel),
            // Delimiters as standalone symbols.
            ("\\lceil", '\u{2308}', C::Open),
            ("\\rVert", '\u{2016}', C::Close),
            ("\\colon", ':', C::Punct),
        ] {
            // Operands on both sides keep binary atoms from demoting.
            let a = atoms(&format!("x{tex} y"));
            assert_eq!(a.len(), 3, "{tex}");
            assert_eq!(a[1].nucleus, Field::Symbol(ch), "{tex}");
            assert_eq!(a[1].class, class, "{tex}");
        }
        // The new rows reach \left and \right.
        let a = atoms("\\left\\lvert x \\right\\rvert");
        let Field::LeftRight { open, close, .. } = &a[0].nucleus else {
            panic!("expected delimited group, got {:?}", a[0].nucleus)
        };
        assert_eq!((*open, *close), (Some('|'), Some('|')));
        let a = atoms("\\left\\uparrow x \\right.");
        let Field::LeftRight { open, .. } = &a[0].nucleus else {
            panic!("expected delimited group, got {:?}", a[0].nucleus)
        };
        assert_eq!(*open, Some('\u{2191}'));
    }

    #[test]
    fn fraction_variants_force_their_style_and_cfrac_is_continued() {
        use crate::mlist::MathStyle;
        let a = atoms("\\tfrac12");
        let Field::Fraction {
            bar,
            style,
            continued,
            ..
        } = &a[0].nucleus
        else {
            panic!("expected a fraction, got {:?}", a[0].nucleus)
        };
        assert_eq!(
            (*bar, *style, *continued),
            (true, Some(MathStyle::Text), false)
        );
        let a = atoms("\\dfrac{a}{b}");
        let Field::Fraction {
            style, continued, ..
        } = &a[0].nucleus
        else {
            panic!()
        };
        assert_eq!((*style, *continued), (Some(MathStyle::Display), false));
        let a = atoms("\\cfrac{1}{2+\\cfrac{1}{3}}");
        let Field::Fraction {
            style,
            continued,
            denominator,
            ..
        } = &a[0].nucleus
        else {
            panic!()
        };
        assert_eq!((*style, *continued), (Some(MathStyle::Display), true));
        assert!(denominator.atoms().any(|at| matches!(
            at.nucleus,
            Field::Fraction {
                continued: true,
                ..
            }
        )));
        // A plain fraction takes the style around it.
        let a = atoms("\\frac12");
        let Field::Fraction { style, .. } = &a[0].nucleus else {
            panic!()
        };
        assert_eq!(*style, None);
        // The binomial variants are the stack in parentheses with the style.
        let a = atoms("\\tbinom{n}{k}");
        let Field::LeftRight { body, .. } = &a[0].nucleus else {
            panic!()
        };
        let inner = body.atoms().next().expect("stack inside");
        let Field::Fraction { bar, style, .. } = &inner.nucleus else {
            panic!()
        };
        assert_eq!((*bar, *style), (false, Some(MathStyle::Text)));
        let a = atoms("\\dbinom{n}{k}");
        let Field::LeftRight { body, .. } = &a[0].nucleus else {
            panic!()
        };
        let Field::Fraction { style, .. } = &body.atoms().next().unwrap().nucleus else {
            panic!()
        };
        assert_eq!(*style, Some(MathStyle::Display));
    }

    #[test]
    fn text_aliases_read_as_text_and_take_one_token_without_a_brace() {
        for cmd in ["textrm", "textnormal", "textup", "hbox", "mbox"] {
            let a = atoms(&format!("\\{cmd}{{as }}x"));
            assert_eq!(a[0].nucleus, Field::Text("as ".into()), "\\{cmd}");
            assert_eq!(a[0].class, AtomClass::Ord);
            assert_eq!(a[1].nucleus, Field::Symbol('x'));
        }
        // TeX's reading: with no brace the argument is the next token.
        let a = atoms("\\mathrm dx");
        assert_eq!(a[0].nucleus, Field::Text("d".into()));
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
        let a = atoms("\\mathrm\\alpha x");
        assert_eq!(a[0].nucleus, Field::Text("\u{03B1}".into()));
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
        let a = atoms("\\hbox z");
        assert_eq!(a[0].nucleus, Field::Text("z".into()));
        // The tie inside text is a space too.
        let a = atoms("\\text{if~}x");
        assert_eq!(a[0].nucleus, Field::Text("if ".into()));
        // A token that is no character leaves the command a literal.
        let a = atoms("\\mathrm^2");
        assert_eq!(a[0].nucleus, Field::Literal("\\mathrm".into()));
        let a = atoms("\\mathrm\\frac12");
        assert_eq!(a[0].nucleus, Field::Literal("\\mathrm".into()));
        assert!(matches!(a[1].nucleus, Field::Fraction { .. }));
        let a = atoms("\\mathrm");
        assert_eq!(a[0].nucleus, Field::Literal("\\mathrm".into()));
    }

    #[test]
    fn class_setters_reclass_their_operand() {
        let a = atoms("\\sum_{n \\mathop = 1}^{3} a_n");
        let sub = a[0].sub.as_ref().expect("a subscript");
        let eq = sub.atoms().nth(1).expect("the = atom");
        assert_eq!(eq.nucleus, Field::Symbol('='));
        assert_eq!(eq.class, AtomClass::Op);
        assert_eq!(eq.limits, Limits::Default);
        let a = atoms("a \\mathrel{R} b");
        assert_eq!(a[1].nucleus, Field::Symbol('R'));
        assert_eq!(a[1].class, AtomClass::Rel);
        // A group of several atoms becomes one atom of the class.
        let a = atoms("\\mathop{max} x");
        let Field::List(inner) = &a[0].nucleus else {
            panic!("expected a list, got {:?}", a[0].nucleus)
        };
        assert_eq!(inner.atoms().count(), 3);
        assert_eq!(a[0].class, AtomClass::Op);
        assert_eq!(a[0].limits, Limits::Default);
        let a = atoms(
            "a \\mathbin{\\ast} b \\mathpunct{,} \\mathopen{[} c \\mathclose{]} \\mathord{+} \\mathinner{x}",
        );
        let classes: Vec<AtomClass> = a.iter().map(|at| at.class).collect();
        assert_eq!(
            classes,
            [
                AtomClass::Ord,
                AtomClass::Bin,
                AtomClass::Ord,
                AtomClass::Punct,
                AtomClass::Open,
                AtomClass::Ord,
                AtomClass::Close,
                AtomClass::Ord,
                AtomClass::Inner,
            ]
        );
        // The span covers the command and its operand.
        let a = atoms("\\mathrel{R}");
        assert_eq!(a[0].span, 0..11);
    }

    #[test]
    fn pmod_and_mod_expand_to_their_text() {
        let a = atoms("a \\pmod{n}");
        assert_eq!(a.len(), 2);
        assert_eq!(a[1].class, AtomClass::Ord);
        let Field::List(inner) = &a[1].nucleus else {
            panic!("expected a list, got {:?}", a[1].nucleus)
        };
        let parts: Vec<(&Field, AtomClass)> =
            inner.atoms().map(|at| (&at.nucleus, at.class)).collect();
        assert_eq!(parts[0].0, &Field::Kern(8.0 / 18.0));
        assert_eq!(parts[1], (&Field::Symbol('('), AtomClass::Open));
        assert_eq!(parts[2], (&Field::Text("mod".into()), AtomClass::Op));
        assert_eq!(parts[3].0, &Field::Kern(6.0 / 18.0));
        assert_eq!(parts[4].0, &Field::Symbol('n'));
        assert_eq!(parts[5], (&Field::Symbol(')'), AtomClass::Close));
        assert_eq!(parts.len(), 6);
        let a = atoms("a \\mod{n}");
        let Field::List(inner) = &a[1].nucleus else {
            panic!()
        };
        let parts: Vec<&Field> = inner.atoms().map(|at| &at.nucleus).collect();
        assert_eq!(parts[0], &Field::Kern(12.0 / 18.0));
        assert_eq!(parts[1], &Field::Text("mod".into()));
        assert_eq!(parts[2], &Field::Kern(6.0 / 18.0));
        assert_eq!(parts[3], &Field::Symbol('n'));
        assert_eq!(parts.len(), 4);
        // The operand's group opens as any group.
        let a = atoms("\\pmod{2\\pi}");
        let Field::List(inner) = &a[0].nucleus else {
            panic!()
        };
        assert_eq!(inner.atoms().count(), 7);
    }

    #[test]
    fn mediawiki_names_resolve_as_texvc_and_katex_read_them() {
        for name in ["arccot", "arccsc", "arcsec", "sen", "sgn"] {
            let a = atoms(&format!("\\{name} x"));
            assert_eq!(a[0].nucleus, Field::Text(name.into()), "\\{name}");
            assert_eq!(a[0].class, AtomClass::Op);
            assert_eq!(a[0].limits, Limits::NoLimits);
        }
        // An alias gives its target's symbol and class.
        let pairs = [
            ("R", "mathbb{R}"),
            ("Complex", "mathbb{C}"),
            ("cnums", "mathbb{C}"),
            ("H", "mathbb{H}"),
            ("infin", "infty"),
            ("rarr", "rightarrow"),
            ("hArr", "Leftrightarrow"),
            ("isin", "in"),
            ("sdot", "cdot"),
            ("lang", "langle"),
            ("part", "partial"),
            ("and", "land"),
            ("empty", "emptyset"),
            ("sect", "S"),
            ("weierp", "wp"),
        ];
        for (alias, target) in pairs {
            let a = atoms(&format!("\\{alias}"));
            let t = atoms(&format!("\\{target}"));
            assert_eq!(
                (a[0].nucleus.clone(), a[0].class),
                (t[0].nucleus.clone(), t[0].class),
                "\\{alias}"
            );
        }
        // The symbols with no earlier name.
        assert_eq!(atoms("\\Bbbk")[0].nucleus, Field::Symbol('\u{1D55C}'));
        assert_eq!(atoms("\\varGamma")[0].nucleus, Field::Symbol('\u{1D6E4}'));
        assert_eq!(atoms("\\varOmega")[0].nucleus, Field::Symbol('\u{1D6FA}'));
        assert_eq!(atoms("\\complement")[0].nucleus, Field::Symbol('\u{2201}'));
        assert_eq!(atoms("\\iiiint")[0].class, AtomClass::Op);
        assert_eq!(atoms("\\Lleftarrow")[0].class, AtomClass::Rel);
        assert_eq!(atoms("\\gt")[0].nucleus, Field::Symbol('>'));
        assert_eq!(atoms("\\lt")[0].class, AtomClass::Rel);
        // \smallint stays out: noad grows every operator in display.
        assert_eq!(
            atoms("\\smallint")[0].nucleus,
            Field::Literal("\\smallint".into())
        );
        // The extension pieces of TeX draw as a bar, which the font stretches.
        assert_eq!(atoms("\\arrowvert")[0].nucleus, Field::Symbol('|'));
        assert_eq!(atoms("\\bracevert")[0].nucleus, Field::Symbol('|'));
        // After \left, \lt and \gt are the angle brackets, as KaTeX reads them.
        let a = atoms("\\left\\lt x \\right\\gt");
        assert!(matches!(
            a[0].nucleus,
            Field::LeftRight {
                open: Some('\u{27E8}'),
                close: Some('\u{27E9}'),
                ..
            }
        ));
        // The delimiters after \left and \right.
        let a = atoms("\\left\\lang x \\right\\rang");
        assert!(matches!(
            a[0].nucleus,
            Field::LeftRight {
                open: Some('\u{27E8}'),
                close: Some('\u{27E9}'),
                ..
            }
        ));
        let a = atoms("\\left\\bracevert x \\right\\Arrowvert");
        assert!(matches!(
            a[0].nucleus,
            Field::LeftRight {
                open: Some('|'),
                close: Some('\u{2016}'),
                ..
            }
        ));
    }

    #[test]
    fn alphabet_aliases_and_bold_italic_map_their_letters() {
        assert_eq!(
            atoms("\\boldsymbol{x}")[0].nucleus,
            Field::Symbol('\u{1D499}')
        );
        assert_eq!(atoms("\\bm{A}")[0].nucleus, Field::Symbol('\u{1D468}'));
        assert_eq!(
            atoms("\\boldsymbol{\\alpha}")[0].nucleus,
            Field::Symbol('\u{1D736}')
        );
        assert_eq!(
            atoms("\\boldsymbol{\\Gamma}")[0].nucleus,
            Field::Symbol('\u{1D6AA}')
        );
        assert_eq!(
            atoms("\\boldsymbol{\\nabla}")[0].nucleus,
            Field::Symbol('\u{1D735}')
        );
        assert_eq!(
            atoms("\\boldsymbol{\\partial}")[0].nucleus,
            Field::Symbol('\u{1D74F}')
        );
        assert_eq!(
            atoms("\\boldsymbol{1}")[0].nucleus,
            Field::Symbol('\u{1D7CF}')
        );
        assert_eq!(atoms("\\boldsymbol{+}")[0].nucleus, Field::Symbol('+'));
        for (alias, target) in [
            ("Bbb", "mathbb"),
            ("bold", "mathbf"),
            ("frak", "mathfrak"),
            ("mathscr", "mathcal"),
            ("scr", "mathcal"),
        ] {
            assert_eq!(
                atoms(&format!("\\{alias}{{R}}"))[0].nucleus,
                atoms(&format!("\\{target}{{R}}"))[0].nucleus,
                "\\{alias}"
            );
        }
        assert_eq!(atoms("\\mathnormal{x}")[0].nucleus, Field::Symbol('x'));
    }

    #[test]
    fn spacing_aliases_give_katex_s_kerns() {
        let kern = |tex: &str| match atoms(tex)[0].nucleus {
            Field::Kern(ems) => ems,
            ref other => panic!("{tex}: {other:?}"),
        };
        assert_eq!(kern("\\enspace"), 0.5);
        assert_eq!(kern("\\thinspace"), 3.0 / 18.0);
        assert_eq!(kern("\\medspace"), 4.0 / 18.0);
        assert_eq!(kern("\\thickspace"), 5.0 / 18.0);
        assert_eq!(kern("\\negthinspace"), -3.0 / 18.0);
        assert_eq!(kern("\\negmedspace"), -4.0 / 18.0);
        assert_eq!(kern("\\negthickspace"), -5.0 / 18.0);
        assert_eq!(kern("\\nobreakspace"), 0.25);
        assert_eq!(kern("\\space"), 0.25);
        // TeX's tie is a space in a formula, not a tilde.
        let a = atoms("a~b");
        assert_eq!(a[1].nucleus, Field::Kern(0.25));
        assert_eq!(a[2].nucleus, Field::Symbol('b'));
    }

    #[test]
    fn literals_lists_what_the_parser_did_not_understand_in_source_order() {
        let l = parse(
            "\\foo + \\frac{\\quuz}{\\sqrt[\\baz]{x}} \\left( \\qux \\right) \\hat{\\quux} \
             \\begin{pmatrix} \\corge & 1 \\end{pmatrix} x^{\\grault}_{\\garply}",
        );
        assert_eq!(
            l.literals(),
            ["\\foo", "\\quuz", "\\baz", "\\qux", "\\quux", "\\corge", "\\grault", "\\garply"]
        );
        assert!(parse("\\frac{1}{2} + \\tfrac{a}{b}").literals().is_empty());
    }

    /// Formulas of the challenge file's math part, as the file has them.
    const FILE_HBOX: &str = r"\Gamma(z) \sim \sqrt{2\pi}z^{z-1/2}e^{-z}\quad\hbox{as }z\to\infty\hbox{ in } \left|\arg(z)\right|<\pi.";
    const FILE_TEXTRM: &str = r"\begin{aligned} a_0 &= c_0 &\\ a_n &= c_n+c_{-n} \qquad &\textrm{for}~ n > 0 \\ b_n &= (c_n-c_{-n})/i \qquad &\textrm{for}~ n > 0 \end{aligned}";
    const FILE_MATHREL: &str = r"\begin{aligned} \oint_{\partial \Sigma} & \mathbf{B}\cdot\mathrm{d}\mathbf{\ell} = \frac{1}{c} \left( 4\pi \iint_\Sigma \mathbf{J}\cdot\mathrm{d}\mathbf{S} + \frac{\mathrel{\mathrm{d} } }{\mathrm{d}t} \iint_\Sigma \mathbf{E}\cdot \mathrm{d}\mathbf{S}\right) \end{aligned}";

    #[test]
    fn the_file_s_formulas_of_category_a_parse_whole() {
        // The shortest formulas of the challenge file's math part for each
        // construct of the category (`target/p348/constructs.md`).
        let whole = [
            "\\tfrac ba",
            "\\zeta(0) = -\\tfrac{1}{2}",
            "(1 + \\tfrac {i\\pi}{n})^n",
            "\\int \\left| f(x)\\right|\\,dx = \\sgn(f(x))g(x)+C,",
            "\\dfrac {1}{\\Gamma (z)}",
            "|c_n|^2 = \\begin{cases} \\dfrac{1}{n^2}, & \\text{for } n \\neq 0, \\\\ 0, & \\text{for } n = 0, \\end{cases}",
            "\\int_0^\\infty \\frac{\\,\\gamma x+\\log\\Gamma(1+x)\\,}{x^{5/2} } \\, \\mathrm d x = \\frac{2\\pi}{3}\\,\\zeta\\left( \\frac{3}{2} \\right)",
            "\\arccot x \\pm \\arccot y",
            "\\arcsec x = -i\\, \\ln \\left(\\frac{1}{x} +i \\sqrt{1 - \\frac{1}{x^2} }\\right)",
            "\\arccsc x = -i\\, \\ln \\left(\\frac{i}{x} + \\sqrt{1 - \\frac{1}{x^2} }\\right)",
            "\\frac{\\zeta(2)}{2} = \\cfrac{1}{v_1 + \\cfrac{1^4}{v_2+\\cfrac{2^4}{v_3+\\cfrac{3^4}{v_4+\\ddots} } } },",
            "\\tbinom{n}{k}",
            "\\Complex^2",
            "E = \\beta mc^2 + c\\left(\\sum_{n \\mathop = 1}^{3}\\alpha_n p_n\\right)",
            FILE_HBOX,
            FILE_TEXTRM,
            FILE_MATHREL,
        ];
        for tex in whole {
            assert_eq!(parse(tex).literals(), Vec::<String>::new(), "{tex}");
        }
        // The `\not` of category D is the one literal left in this formula.
        assert_eq!(
            parse("\\theta \\not\\equiv 0 \\pmod{2\\pi}.").literals(),
            ["\\not"]
        );
    }

    #[test]
    fn a_text_command_and_a_delimiter_expand_a_macro_first() {
        let a = atoms("\\newcommand{\\C}{x}\\mathrm\\C");
        assert_eq!(a[0].nucleus, Field::Text("x".into()));
        assert_eq!(a.len(), 1, "the macro is read once");
        let a = atoms("\\renewcommand{\\alpha}{\\beta}\\mathrm\\alpha");
        assert_eq!(a[0].nucleus, Field::Text("\u{03B2}".into()));
        // A macro that expands to one character reads it.
        let a = atoms("\\newcommand{\\g}{y}\\text\\g");
        assert_eq!(a[0].nucleus, Field::Text("y".into()));
        // The delimiter after \left expands a macro too, once.
        let a = atoms("\\newcommand{\\lang}{[}\\left\\lang x \\right]");
        assert_eq!(a.len(), 1);
        assert!(matches!(
            a[0].nucleus,
            Field::LeftRight {
                open: Some('['),
                close: Some(']'),
                ..
            }
        ));
        // TeX's tilde accent inside text keeps its tilde; a tie is a space.
        let a = atoms("\\text{a~b\\~c}");
        assert_eq!(a[0].nucleus, Field::Text("a b\\~c".into()));
        // The tie as the one token is a space too.
        let a = atoms("\\mathrm~x");
        assert_eq!(a[0].nucleus, Field::Text(" ".into()));
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
    }

    #[test]
    fn a_class_setter_keeps_a_group_unless_it_is_one_plain_atom() {
        // TeX's rule: a group of one Ord atom without scripts is the atom
        // itself; any other group stays a list under the new class, so
        // its scripts stay inside and an operator in it keeps its size.
        let a = atoms("\\mathop{x^2}");
        let Field::List(inner) = &a[0].nucleus else {
            panic!("{:?}", a[0].nucleus)
        };
        assert!(a[0].sup.is_none());
        assert!(inner.atoms().next().unwrap().sup.is_some());
        assert_eq!(a[0].class, AtomClass::Op);
        let a = atoms("\\mathord{\\sum}");
        let Field::List(inner) = &a[0].nucleus else {
            panic!("{:?}", a[0].nucleus)
        };
        assert_eq!(inner.atoms().next().unwrap().class, AtomClass::Op);
        assert_eq!(a[0].class, AtomClass::Ord);
        let a = atoms("\\mathrel{R}");
        assert_eq!(a[0].nucleus, Field::Symbol('R'));
        // One token without braces is the atom itself, reclassed.
        let a = atoms("a \\mathbin\\sum b");
        assert_eq!(a[1].nucleus, Field::Symbol('\u{2211}'));
        assert_eq!(a[1].class, AtomClass::Bin);
        let a = atoms("\\mathop =");
        assert_eq!(a[0].nucleus, Field::Symbol('='));
        assert_eq!(a[0].class, AtomClass::Op);
    }

    #[test]
    fn a_macro_that_expands_to_nothing_leaves_an_empty_operand() {
        // The read of Task 349: `script_operand` panicked when the atom it
        // had peeked expanded to nothing or to a marker.
        let a = atoms("a\\newcommand{\\e}{}\\frac1\\e");
        let Field::Fraction { denominator, .. } = &a[1].nucleus else {
            panic!("{:?}", a[1].nucleus)
        };
        assert!(denominator.0.is_empty());
        let a = atoms("a\\newcommand{\\e}{}\\mathop\\e");
        assert_eq!(a[1].class, AtomClass::Op);
        assert_eq!(a[1].nucleus, Field::List(MathList::default()));
        let a = atoms("{\\newcommand{\\e}{}\\pmod\\e} x");
        assert_eq!(a.len(), 2);
        let a = atoms("\\newcommand{\\e}{^}\\mathrel\\e x");
        assert_eq!(a[0].class, AtomClass::Rel);
        assert!(
            a[0].sup.is_some(),
            "the marker the macro left attaches as a script"
        );
        // The fuzzer's cases: an empty macro before a row break keeps the
        // break for the matrix, and before \right keeps the closer.
        let a = atoms("\\left( \\newcommand{\\AA}{} \\AA A \\sqrt\\AA \\right(");
        assert!(matches!(
            a[0].nucleus,
            Field::LeftRight {
                close: Some('('),
                ..
            }
        ));
        assert!(
            parse("\\left( \\newcommand{\\AA}{} \\AA A \\sqrt\\AA \\right(")
                .literals()
                .is_empty()
        );
        // The degree of a root is read by its tokens: the bracket stays
        // the degree's whatever an empty macro before it leaves.
        let a = atoms("\\sqrt[\\newcommand{\\AA}{} \\AA A \\sqrt\\AA]{a}");
        assert_eq!(a.len(), 1);
        let Field::Radical { degree, radicand } = &a[0].nucleus else {
            panic!("{:?}", a[0].nucleus)
        };
        assert_eq!(degree.as_ref().unwrap().atoms().count(), 2);
        assert_eq!(radicand.atoms().next().unwrap().nucleus, Field::Symbol('a'));
        assert!(parse("\\sqrt[\\newcommand{\\AA}{} \\AA A \\sqrt\\AA]{a}")
            .literals()
            .is_empty());
        // A marker as the operand: an empty operand, the marker kept.
        let a = atoms("{x^}y");
        assert_eq!(a.len(), 2);
        assert_eq!(a[1].nucleus, Field::Symbol('y'));
        let a = atoms(
            "\\begin{matrix} A & \\newcommand{\\AA}{} \\AA A \\sqrt\\AA \\\\ a & a \\end{matrix}",
        );
        let Field::Table { rows, .. } = &a[0].nucleus else {
            panic!("{:?}", a[0].nucleus)
        };
        assert_eq!(rows.len(), 2);
        assert!(parse(
            "\\begin{matrix} A & \\newcommand{\\AA}{} \\AA A \\sqrt\\AA \\\\ a & a \\end{matrix}"
        )
        .literals()
        .is_empty());
        // TeX's reading of the empty macro before each argument: each
        // command takes the next one, and the chain nests on the x.
        let a = atoms("\\newcommand{\\e}{}\\sqrt\\e \\mathbf\\e \\hat\\e x^\\e");
        assert_eq!(a.len(), 1);
        assert!(matches!(a[0].nucleus, Field::Radical { .. }));
        assert_eq!(a[0].sup, Some(MathList::default()));
    }

    #[test]
    fn a_text_command_inside_a_macro_body_keeps_its_text() {
        // The tokens a macro splices carry the call's span; the text is
        // read where the body is in the source, spaces included.
        let a = atoms("\\newcommand{\\iff}{\\text{if and only if}}\\iff");
        assert_eq!(a[0].nucleus, Field::Text("if and only if".into()));
        let a = atoms("\\newcommand{\\g}{{ab}}\\text\\g");
        assert_eq!(a[0].nucleus, Field::Text("ab".into()));
        // The call's span stays the atom's.
        assert_eq!(a[0].span, 21..28);
        // An argument mixed into the body's text keeps the body's spaces
        // around it and its own inside; an empty one leaves its space.
        let a = atoms("\\newcommand{\\t}[1]{\\text{#1 is}}\\t{x}");
        assert_eq!(a[0].nucleus, Field::Text("x is".into()));
        let a = atoms("\\newcommand{\\t}[1]{\\text{is #1}}\\t{x}");
        assert_eq!(a[0].nucleus, Field::Text("is x".into()));
        let a = atoms("\\newcommand{\\t}[1]{\\text{ #1 }}\\t{m s}");
        assert_eq!(a[0].nucleus, Field::Text(" m s ".into()));
        let a = atoms("\\newcommand{\\t}[2]{\\text{#1#2 and #1 #2}}\\t{a}{b}");
        assert_eq!(a[0].nucleus, Field::Text("ab and ab".into()));
        let a = atoms("\\newcommand{\\t}[1]{\\text{#1 is}}\\t{}");
        assert_eq!(a[0].nucleus, Field::Text(" is".into()));
        // TeX's `##` in a body is one `#`.
        let a = atoms("\\newcommand{\\t}{\\text{a ## b}}\\t");
        assert_eq!(a[0].nucleus, Field::Text("a # b".into()));
        // A body used twice reads the same text twice.
        let a = atoms("\\newcommand{\\m}{\\mathrm{d}x}\\m + \\m");
        assert_eq!(a[0].nucleus, Field::Text("d".into()));
        assert_eq!(a[3].nucleus, Field::Text("d".into()));
    }

    #[test]
    fn sized_delimiters_parse_with_their_size_and_class() {
        let a = atoms("\\bigl( x \\bigr)");
        assert_eq!(
            a[0].nucleus,
            Field::Sized {
                ch: Some('('),
                size: 1
            }
        );
        assert_eq!(a[0].class, AtomClass::Open);
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
        assert_eq!(
            a[2].nucleus,
            Field::Sized {
                ch: Some(')'),
                size: 1
            }
        );
        assert_eq!(a[2].class, AtomClass::Close);
        for (word, size) in [("big", 1), ("Big", 2), ("bigg", 3), ("Bigg", 4)] {
            let a = atoms(&format!("\\{word}|"));
            assert_eq!(
                a[0].nucleus,
                Field::Sized {
                    ch: Some('|'),
                    size
                },
                "\\{word}"
            );
            assert_eq!(a[0].class, AtomClass::Ord);
            let a = atoms(&format!("\\{word}m\\|"));
            assert_eq!(
                a[0].nucleus,
                Field::Sized {
                    ch: Some('\u{2016}'),
                    size
                },
                "\\{word}m"
            );
            assert_eq!(a[0].class, AtomClass::Rel);
            let a = atoms(&format!("\\{word}l[ \\{word}r]"));
            assert_eq!(a[0].class, AtomClass::Open);
            assert_eq!(a[1].class, AtomClass::Close);
            assert_eq!(
                a[1].nucleus,
                Field::Sized {
                    ch: Some(']'),
                    size
                }
            );
        }
        // The delimiter commands of \left, a slash and a brace.
        assert_eq!(
            atoms("\\bigg\\langle")[0].nucleus,
            Field::Sized {
                ch: Some('\u{27E8}'),
                size: 3
            }
        );
        assert_eq!(
            atoms("\\big/")[0].nucleus,
            Field::Sized {
                ch: Some('/'),
                size: 1
            }
        );
        assert_eq!(
            atoms("\\Bigl\\{")[0].nucleus,
            Field::Sized {
                ch: Some('{'),
                size: 2
            }
        );
        assert_eq!(
            atoms("\\Bigg\\Updownarrow")[0].nucleus,
            Field::Sized {
                ch: Some('\u{21D5}'),
                size: 4
            }
        );
        // A dot is an empty delimiter of the size, TeX's invisible box.
        let a = atoms("\\big. x");
        assert_eq!(a[0].nucleus, Field::Sized { ch: None, size: 1 });
        assert_eq!(a[0].class, AtomClass::Ord);
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
        // The delimiter may be braced, as TeX's \big takes an argument.
        let a = atoms("\\big{(} x");
        assert_eq!(
            a[0].nucleus,
            Field::Sized {
                ch: Some('('),
                size: 1
            }
        );
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
        // TeX's `<` and `>` delimiters are the angle brackets.
        assert_eq!(
            atoms("\\big<")[0].nucleus,
            Field::Sized {
                ch: Some('\u{27E8}'),
                size: 1
            }
        );
        assert_eq!(
            atoms("\\Bigr>")[0].nucleus,
            Field::Sized {
                ch: Some('\u{27E9}'),
                size: 2
            }
        );
        // A character that is no delimiter in TeX leaves the word a literal
        // and parses as itself.
        let a = atoms("\\big x");
        assert_eq!(a[0].nucleus, Field::Literal("\\big".into()));
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
        let a = atoms("\\big+ y");
        assert_eq!(a[0].nucleus, Field::Literal("\\big".into()));
        assert_eq!(a[1].nucleus, Field::Symbol('+'));
        // No delimiter leaves the word a literal, and what follows parses on.
        let a = atoms("\\big{x}");
        assert_eq!(a[0].nucleus, Field::Literal("\\big".into()));
        let Field::List(inner) = &a[1].nucleus else {
            panic!("{:?}", a[1].nucleus)
        };
        assert_eq!(inner.atoms().next().unwrap().nucleus, Field::Symbol('x'));
        assert_eq!(atoms("\\bigr")[0].nucleus, Field::Literal("\\bigr".into()));
        let a = atoms("\\Big\\frac12");
        assert_eq!(a[0].nucleus, Field::Literal("\\Big".into()));
        assert!(matches!(a[1].nucleus, Field::Fraction { .. }));
        // Scripts attach to the delimiter.
        let a = atoms("\\bigr)^2");
        assert!(a[0].sup.is_some());
        assert_eq!(a[0].span, 0..8);
        assert_eq!(a[0].nucleus_span, 0..6);
    }

    #[test]
    fn the_delimiter_reader_takes_tex_s_delimiters_alone() {
        // After \left, a letter is no delimiter: TeX inserts `.` and the
        // letter goes to the body.
        let a = atoms("\\left x \\right.");
        let Field::LeftRight { open, close, body } = &a[0].nucleus else {
            panic!("{:?}", a[0].nucleus)
        };
        assert_eq!((*open, *close), (None, None));
        assert_eq!(body.atoms().next().unwrap().nucleus, Field::Symbol('x'));
        // `<` and `>` are the angle brackets after \left too.
        let a = atoms("\\left< x \\right>");
        let Field::LeftRight { open, close, .. } = &a[0].nucleus else {
            panic!("{:?}", a[0].nucleus)
        };
        assert_eq!((*open, *close), (Some('\u{27E8}'), Some('\u{27E9}')));
        // A delimiter character written directly is one.
        let a = atoms("\\left\u{27E8} x \\right\u{2016}");
        let Field::LeftRight { open, close, .. } = &a[0].nucleus else {
            panic!("{:?}", a[0].nucleus)
        };
        assert_eq!((*open, *close), (Some('\u{27E8}'), Some('\u{2016}')));
    }

    #[test]
    fn a_bin_before_a_closer_or_a_relation_is_ordinary() {
        // TeX's rule 6: a binary operator followed by a relation, a closer
        // or punctuation, or ending the list, is an ordinary atom.
        assert_eq!(atoms("x+)")[1].class, AtomClass::Ord);
        assert_eq!(atoms("x+\\bigr)")[1].class, AtomClass::Ord);
        assert_eq!(atoms("x+=y")[1].class, AtomClass::Ord);
        assert_eq!(atoms("x+,y")[1].class, AtomClass::Ord);
        assert_eq!(atoms("x+")[1].class, AtomClass::Ord);
        assert_eq!(atoms("x+\\,")[1].class, AtomClass::Ord);
        assert_eq!(atoms("x+y")[1].class, AtomClass::Bin);
        assert_eq!(atoms("x+\\bigl(y\\bigr)")[1].class, AtomClass::Bin);
    }

    #[test]
    fn the_file_s_formulas_of_category_b_parse_whole() {
        let whole = [
            r"\sum_{n=2}^\infty\bigl(\zeta(n)-1\bigr) = 1",
            r"f(x)={\ln}\bigl(1+(\cos x-1)\bigr),",
            r"\sqrt{b^2 - 4ac}\big/ 2a",
            r"\Gamma(-x) = \frac{1}{\Gamma(x+1)}\frac{\pi}{\sin\big(\pi(x+1)\big)},",
            r"j_{8A'}(\tau) = -j_{8A}\Big(\tau+\tfrac12\Big)",
            r"\frac{1}{P}\int_{P} |s(x)|^2 \, dx = \sum_{n=-\infty}^\infty \Bigl|S[n]\Bigr|^2.",
            r"J_\nu(z)=\sum_{k=0}^\infty \frac{(-1)^k}{\Gamma(k+\nu+1)k!}\bigg(\frac{z}{2}\bigg)^{2k+\nu}",
            r"\biggl(\sum_{n\ne 0}\bigl|S[n]\bigr|\biggr)^2\le \sum_{n\ne 0}\frac1{n^2}\cdot\sum_{n\ne 0}\bigl|nS[n]\bigr|^2.",
            r"\zeta (s)=\frac{1}{s-1}\sum_{k=0}^\infty \biggl(\prod_{i=1}^{k} (i-\frac{s}{2})\biggl) \frac{A_{k} }{k!}",
            r"\begin{aligned} \|f\|^2 & = \int_0^1 \vartheta^2 \, d\vartheta = \frac{1}{3} \\ \langle f, e_k\rangle & = \int_0^1 \vartheta e^{-2\pi\imath k\vartheta} \, d\vartheta = \Biggl\{\begin{array}{ll} \frac{1}{2}, & k = 0 \\ -\frac{1}{2\pi\imath k} & k \neq 0, \end{array} \end{aligned}",
            r"\begin{array}{rlc} \mathsf{Time\ domain} & s & = s_{\mathrm{RE} } \\ &\Bigg\Updownarrow\mathcal{F} & \Bigg\Updownarrow\mathcal{F} \end{array}",
        ];
        for tex in whole {
            assert_eq!(parse(tex).literals(), Vec::<String>::new(), "{tex}");
        }
    }

    /// The atoms inside a braced group's atom.
    fn inner(atom: &Atom) -> Vec<Atom> {
        let Field::List(list) = &atom.nucleus else {
            panic!("{:?}", atom.nucleus)
        };
        list.atoms().cloned().collect()
    }

    #[test]
    fn infix_fractions_take_the_group_s_two_halves() {
        // TeX's \over: what came before it in the group is the numerator,
        // the rest of the group the denominator, one atom in their place.
        let a = atoms("{a+b \\over c} d");
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].span, 0..13);
        assert_eq!(a[1].nucleus, Field::Symbol('d'));
        let g = inner(&a[0]);
        assert_eq!(g.len(), 1);
        let Field::Fraction {
            numerator,
            denominator,
            bar,
            style,
            ..
        } = &g[0].nucleus
        else {
            panic!("{:?}", g[0].nucleus)
        };
        assert!(*bar);
        assert_eq!(*style, None);
        assert_eq!(numerator.atoms().count(), 3);
        assert_eq!(denominator.atoms().count(), 1);
        assert_eq!(g[0].class, AtomClass::Inner);
        assert_eq!(g[0].span, 1..12);
        // The top level is a group too.
        let a = atoms("a \\over b");
        assert_eq!(a.len(), 1);
        assert!(matches!(a[0].nucleus, Field::Fraction { bar: true, .. }));
        // \atop has no bar; an empty half is an empty list.
        let g = inner(&atoms("{\\atop b}")[0]);
        let Field::Fraction { numerator, bar, .. } = &g[0].nucleus else {
            panic!("{:?}", g[0].nucleus)
        };
        assert!(!*bar);
        assert!(numerator.0.is_empty());
        let g = inner(&atoms("{a \\over}")[0]);
        let Field::Fraction { denominator, .. } = &g[0].nucleus else {
            panic!("{:?}", g[0].nucleus)
        };
        assert!(denominator.0.is_empty());
        // \choose is \binom; \brace and \brack take their delimiters.
        for (word, open, close) in [
            ("choose", '(', ')'),
            ("brace", '{', '}'),
            ("brack", '[', ']'),
        ] {
            let g = inner(&atoms(&format!("{{n \\{word} k}}"))[0]);
            let Field::LeftRight {
                open: o,
                close: c,
                body,
            } = &g[0].nucleus
            else {
                panic!("{:?}", g[0].nucleus)
            };
            assert_eq!((*o, *c), (Some(open), Some(close)), "\\{word}");
            let stack = body.atoms().next().unwrap();
            assert!(matches!(stack.nucleus, Field::Fraction { bar: false, .. }));
        }
        // A second infix word in the group is a literal in the denominator.
        let g = inner(&atoms("{a \\over b \\over c}")[0]);
        assert_eq!(g.len(), 1);
        let Field::Fraction { denominator, .. } = &g[0].nucleus else {
            panic!("{:?}", g[0].nucleus)
        };
        let den: Vec<&Atom> = denominator.atoms().collect();
        assert_eq!(den.len(), 3);
        assert_eq!(den[1].nucleus, Field::Literal("\\over".into()));
        // Every group kind: a radicand, a degree, a \left body, a cell, a script.
        let a = atoms("\\sqrt{a \\over b}");
        let Field::Radical { radicand, .. } = &a[0].nucleus else {
            panic!("{:?}", a[0].nucleus)
        };
        assert!(matches!(
            radicand.atoms().next().unwrap().nucleus,
            Field::Fraction { .. }
        ));
        let a = atoms("\\sqrt[a \\over b]{c}");
        let Field::Radical { degree, .. } = &a[0].nucleus else {
            panic!("{:?}", a[0].nucleus)
        };
        let degree = degree.as_ref().unwrap();
        assert!(matches!(
            degree.atoms().next().unwrap().nucleus,
            Field::Fraction { .. }
        ));
        let a = atoms("\\left( a \\over b \\right) c");
        let Field::LeftRight { body, .. } = &a[0].nucleus else {
            panic!("{:?}", a[0].nucleus)
        };
        assert_eq!(body.atoms().count(), 1);
        assert!(matches!(
            body.atoms().next().unwrap().nucleus,
            Field::Fraction { .. }
        ));
        assert_eq!(a[1].nucleus, Field::Symbol('c'));
        let a = atoms("\\begin{matrix} a \\over b & c \\\\ d & e \\end{matrix}");
        let Field::Table { rows, .. } = &a[0].nucleus else {
            panic!("{:?}", a[0].nucleus)
        };
        assert_eq!(rows.len(), 2);
        assert!(matches!(
            rows[0][0].atoms().next().unwrap().nucleus,
            Field::Fraction { .. }
        ));
        assert_eq!(
            rows[0][1].atoms().next().unwrap().nucleus,
            Field::Symbol('c')
        );
        let a = atoms("x^{a \\over b}");
        let sup = a[0].sup.as_ref().unwrap();
        assert!(matches!(
            sup.atoms().next().unwrap().nucleus,
            Field::Fraction { .. }
        ));
        // Without a brace the word is no operand: the script is empty and
        // the word acts on the group, TeX's missing argument.
        let a = atoms("x^\\over y");
        assert_eq!(a.len(), 1);
        let Field::Fraction { numerator, .. } = &a[0].nucleus else {
            panic!("{:?}", a[0].nucleus)
        };
        let x = numerator.atoms().next().unwrap();
        assert_eq!(x.nucleus, Field::Symbol('x'));
        assert!(x.sup.as_ref().unwrap().0.is_empty());
        // An empty macro before the word leaves a radical of nothing.
        let a = atoms("{\\newcommand{\\e}{} \\sqrt\\e \\over b}");
        let g = inner(&a[0]);
        let Field::Fraction { numerator, .. } = &g[0].nucleus else {
            panic!("{:?}", g[0].nucleus)
        };
        assert!(matches!(
            numerator.atoms().next().unwrap().nucleus,
            Field::Radical { .. }
        ));
    }

    #[test]
    fn font_switches_restyle_the_rest_of_the_group() {
        let a = atoms("{\\bf x} y");
        assert_eq!(
            inner(&a[0])[0].nucleus,
            Field::Symbol(map_alphabet("mathbf", 'x'))
        );
        assert_eq!(a[1].nucleus, Field::Symbol('y'));
        // From the switch to the end of the group, scripts included.
        let g = inner(&atoms("{a \\bf b c^d}")[0]);
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].nucleus, Field::Symbol('a'));
        assert_eq!(g[1].nucleus, Field::Symbol(map_alphabet("mathbf", 'b')));
        assert_eq!(g[2].nucleus, Field::Symbol(map_alphabet("mathbf", 'c')));
        let sup = g[2].sup.as_ref().unwrap();
        assert_eq!(
            sup.atoms().next().unwrap().nucleus,
            Field::Symbol(map_alphabet("mathbf", 'd'))
        );
        // \rm sets the Latin letters upright, as \mathrm does.
        let a = atoms("{\\rm d}x");
        assert_eq!(inner(&a[0])[0].nucleus, Field::Text("d".into()));
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
        let g = inner(&atoms("{\\rm \\alpha 2}")[0]);
        assert_eq!(g[0].nucleus, Field::Symbol('\u{03B1}'));
        assert_eq!(g[1].nucleus, Field::Symbol('2'));
        // A later switch replaces the earlier one.
        let g = inner(&atoms("{\\bf a \\rm b}")[0]);
        assert_eq!(g[0].nucleus, Field::Symbol(map_alphabet("mathbf", 'a')));
        assert_eq!(g[1].nucleus, Field::Text("b".into()));
        for (word, alphabet) in [
            ("it", "mathit"),
            ("sf", "mathsf"),
            ("tt", "mathtt"),
            ("cal", "mathcal"),
        ] {
            let g = inner(&atoms(&format!("{{\\{word} A}}"))[0]);
            assert_eq!(
                g[0].nucleus,
                Field::Symbol(map_alphabet(alphabet, 'A')),
                "\\{word}"
            );
        }
        // A switch before \over restyles both halves.
        let g = inner(&atoms("{\\bf a \\over b}")[0]);
        let Field::Fraction {
            numerator,
            denominator,
            ..
        } = &g[0].nucleus
        else {
            panic!("{:?}", g[0].nucleus)
        };
        assert_eq!(
            numerator.atoms().next().unwrap().nucleus,
            Field::Symbol(map_alphabet("mathbf", 'a'))
        );
        assert_eq!(
            denominator.atoms().next().unwrap().nucleus,
            Field::Symbol(map_alphabet("mathbf", 'b'))
        );
        // A switch with nothing after it adds nothing.
        let a = atoms("{a \\bf} b");
        assert_eq!(a.len(), 2);
        assert_eq!(inner(&a[0]).len(), 1);
        // A font switch holds to the end of the group, across \over: TeX's
        // font is a state, not a noad.
        let g = inner(&atoms("{a \\bf b \\over c}")[0]);
        let Field::Fraction {
            numerator,
            denominator,
            ..
        } = &g[0].nucleus
        else {
            panic!("{:?}", g[0].nucleus)
        };
        let num: Vec<&Atom> = numerator.atoms().collect();
        assert_eq!(num[0].nucleus, Field::Symbol('a'));
        assert_eq!(num[1].nucleus, Field::Symbol(map_alphabet("mathbf", 'b')));
        assert_eq!(
            denominator.atoms().next().unwrap().nucleus,
            Field::Symbol(map_alphabet("mathbf", 'c'))
        );
        // A script right after a font switch lands on the atom before it.
        let a = atoms("\\sum \\bf _i x");
        assert_eq!(a.len(), 2);
        assert!(a[0].sub.is_some());
        assert_eq!(a[1].nucleus, Field::Symbol(map_alphabet("mathbf", 'x')));
    }

    /// The list inside a braced group's atom.
    fn inner_list(atom: &Atom) -> MathList {
        let Field::List(list) = &atom.nucleus else {
            panic!("{:?}", atom.nucleus)
        };
        list.clone()
    }

    #[test]
    fn style_switches_are_noads_of_their_list() {
        // TeX's style noad: an item of the list, not an atom, that sets the
        // style for the rest of the group.
        let l = parse("a \\textstyle b c");
        assert_eq!(l.0.len(), 4);
        assert!(matches!(l.0[1], Noad::Style(MathStyle::Text)));
        let a: Vec<&Atom> = l.atoms().collect();
        assert_eq!(a.len(), 3);
        assert_eq!(a[1].nucleus, Field::Symbol('b'));
        for (word, style) in [
            ("displaystyle", MathStyle::Display),
            ("scriptstyle", MathStyle::Script),
            ("scriptscriptstyle", MathStyle::ScriptScript),
        ] {
            let g = inner_list(&atoms(&format!("{{\\{word} x}}"))[0]);
            assert!(matches!(g.0[0], Noad::Style(s) if s == style), "\\{word}");
            assert_eq!(g.atoms().count(), 1);
        }
        // The switch is transparent to the binary rules.
        assert_eq!(atoms("a \\displaystyle + b")[1].class, AtomClass::Bin);
        assert_eq!(atoms("a + \\displaystyle")[1].class, AtomClass::Ord);
        // A script right after a style switch hangs on an empty atom, as
        // TeX inserts one.
        let a = atoms("a \\displaystyle ^2");
        assert_eq!(a.len(), 2);
        assert_eq!(a[1].nucleus, Field::Empty);
        assert!(a[1].sup.is_some());
        // Before \over the switch stays in the numerator's list, and the
        // fraction keeps the style around it: TeX, not \dfrac.
        let g = inner_list(&atoms("{\\displaystyle a \\over b}")[0]);
        let Noad::Atom(frac) = &g.0[0] else {
            panic!("{:?}", g.0[0])
        };
        let Field::Fraction {
            numerator,
            denominator,
            style,
            ..
        } = &frac.nucleus
        else {
            panic!("{:?}", frac.nucleus)
        };
        assert_eq!(*style, None);
        assert_eq!(numerator.0.len(), 2);
        assert!(matches!(numerator.0[0], Noad::Style(MathStyle::Display)));
        assert_eq!(numerator.atoms().count(), 1);
        assert_eq!(denominator.atoms().count(), 1);
        // In a script's operand.
        let a = atoms("x^{\\displaystyle y}");
        let sup = a[0].sup.as_ref().unwrap();
        assert!(matches!(sup.0[0], Noad::Style(MathStyle::Display)));
    }

    #[test]
    fn a_marker_word_acts_after_a_definition_or_an_expansion() {
        let a = atoms("\\newcommand{\\e}{x} \\over b");
        assert!(matches!(a[0].nucleus, Field::Fraction { .. }));
        let g = inner(&atoms("\\newcommand{\\o}{\\over} {a \\o b}")[0]);
        assert!(matches!(g[0].nucleus, Field::Fraction { .. }));
        let g = inner(&atoms("{\\newcommand{\\e}{} a \\e \\over b}")[0]);
        assert!(matches!(g[0].nucleus, Field::Fraction { .. }));
        let l = parse("\\newcommand{\\s}[1]{\\displaystyle #1} \\s{x} y");
        assert!(matches!(l.0[0], Noad::Style(MathStyle::Display)));
        assert_eq!(l.atoms().count(), 2);
        // The infix atom's span covers its items whatever their order in
        // the source: a macro's arguments swapped.
        let tex = "\\newcommand{\\o}[2]{#2 \\over #1} \\o{a}{b}";
        let a = atoms(tex);
        assert!(matches!(a[0].nucleus, Field::Fraction { .. }));
        assert!(a[0].span.start <= a[0].span.end && a[0].span.end <= tex.len());
    }

    #[test]
    fn a_script_first_in_a_left_body_or_a_degree_hangs_on_an_empty_atom() {
        // TeX's implicit empty group, in every reader of a group.
        let a = atoms("\\left( ^2 \\right)");
        let Field::LeftRight { body, .. } = &a[0].nucleus else {
            panic!("{:?}", a[0].nucleus)
        };
        let first = body.atoms().next().unwrap();
        assert_eq!(first.nucleus, Field::Empty);
        assert!(first.sup.is_some());
        let a = atoms("\\left( a \\over ^2 \\right)");
        let Field::LeftRight { body, .. } = &a[0].nucleus else {
            panic!("{:?}", a[0].nucleus)
        };
        let Field::Fraction { denominator, .. } = &body.atoms().next().unwrap().nucleus else {
            panic!("{:?}", body.0)
        };
        let first = denominator.atoms().next().unwrap();
        assert_eq!(first.nucleus, Field::Empty);
        assert!(first.sup.is_some());
        let a = atoms("\\sqrt[^2 c");
        let Field::Radical { degree, .. } = &a[0].nucleus else {
            panic!("{:?}", a[0].nucleus)
        };
        let first = degree.as_ref().unwrap().atoms().next().unwrap();
        assert_eq!(first.nucleus, Field::Empty);
        assert!(first.sup.is_some());
    }

    #[test]
    fn the_file_s_formulas_of_category_c_parse_whole() {
        let whole = [
            r"\int {1 \over x}\,dx = \ln \left|x \right| + C",
            r"p(t) = { {2m + 1} \choose 1}t^m - { {2m + 1} \choose 3}t^{m - 1} \pm \cdots + (-1)^m{ {2m+1} \choose {2m + 1} }.",
            r"\frac{\sin (nx)}{\sin^n x} = \Bigg( {n \choose 1} \cot^{n - 1} x - {n \choose 3} \cot^{n - 3} x \pm \cdots \Bigg).",
            r"\psi( {\bf x},t)=\sqrt{\rho({\bf x},t)}\exp\left(\frac{i}{\hbar} S({\bf x},t)\right)",
            r"\mathbf{J} \cdot {\rm d}\mathbf{S} = - I_{\partial \Omega}.",
        ];
        for tex in whole {
            assert_eq!(parse(tex).literals(), Vec::<String>::new(), "{tex}");
        }
    }

    #[test]
    fn mathrm_and_operatorname_render_upright() {
        let a = atoms("\\mathrm{d}x");
        assert_eq!(a[0].nucleus, Field::Text("d".into()));
        assert_eq!(a[0].class, AtomClass::Ord);
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
        // With no brace the argument is the next token, TeX's reading.
        let a = atoms("\\mathrm x");
        assert_eq!(a[0].nucleus, Field::Text("x".into()));
        let a = atoms("\\operatorname{Var}(X)");
        assert_eq!(a[0].nucleus, Field::Text("Var".into()));
        assert_eq!(a[0].class, AtomClass::Op);
        assert_eq!(a[0].limits, Limits::NoLimits);
        let a = atoms("\\operatorname*{argmax}_x");
        assert_eq!(a[0].limits, Limits::Default);
        let a = atoms("a\\bmod b");
        assert_eq!(a[1].nucleus, Field::Text("mod".into()));
        assert_eq!(a[1].class, AtomClass::Bin);
    }

    #[test]
    fn wide_arrow_and_dotted_accents_join_the_family() {
        let a = atoms("\\overrightarrow{AB}");
        let Field::Accent {
            accent,
            stretch,
            base,
        } = &a[0].nucleus
        else {
            panic!("expected accent, got {:?}", a[0].nucleus)
        };
        assert_eq!((*accent, *stretch), ('\u{20D7}', true));
        assert_eq!(base.atoms().count(), 2);
        let a = atoms("\\dddot{x}");
        let Field::Accent { accent, .. } = &a[0].nucleus else {
            panic!("expected accent, got {:?}", a[0].nucleus)
        };
        assert_eq!(*accent, '\u{20DB}');
    }

    #[test]
    fn newcommand_defines_and_expands() {
        let a = atoms("\\newcommand{\\R}{\\mathbb{R}}\\R");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].nucleus, Field::Symbol('\u{211D}'));
        // The unbraced name form; a definition alone produces nothing.
        assert!(atoms("\\newcommand\\half{\\frac{1}{2}}").is_empty());
        // Macros expand inside environment cells.
        let a = atoms("\\newcommand{\\f}{x}\\begin{matrix} \\f \\end{matrix}");
        let Field::Table { rows, .. } = &a[0].nucleus else {
            panic!("expected table, got {:?}", a[0].nucleus)
        };
        assert_eq!(
            rows[0][0].atoms().next().unwrap().nucleus,
            Field::Symbol('x')
        );
    }

    #[test]
    fn macro_arguments_substitute() {
        let a = atoms("\\newcommand{\\avg}[1]{\\frac{#1}{2}}\\avg{x+y}");
        assert_eq!(a.len(), 1);
        let Field::Fraction {
            numerator,
            denominator,
            ..
        } = &a[0].nucleus
        else {
            panic!("expected fraction, got {:?}", a[0].nucleus)
        };
        assert_eq!(numerator.atoms().count(), 3);
        assert_eq!(
            denominator.atoms().next().unwrap().nucleus,
            Field::Symbol('2')
        );
        // Two arguments land in order.
        let a = atoms("\\newcommand{\\pair}[2]{(#1,#2)}\\pair{a}{b}");
        let nuclei: Vec<Field> = a.iter().map(|at| at.nucleus.clone()).collect();
        assert_eq!(
            nuclei,
            vec![
                Field::Symbol('('),
                Field::Symbol('a'),
                Field::Symbol(','),
                Field::Symbol('b'),
                Field::Symbol(')'),
            ]
        );
        // A braced argument keeps its own nested braces balanced.
        let a = atoms("\\newcommand{\\avg}[1]{\\frac{#1}{2}}\\avg{\\frac{a}{b}}");
        let Field::Fraction { numerator, .. } = &a[0].nucleus else {
            panic!("expected fraction, got {:?}", a[0].nucleus)
        };
        assert_eq!(numerator.atoms().count(), 1);
        assert!(matches!(
            numerator.atoms().next().unwrap().nucleus,
            Field::Fraction { .. }
        ));
        // A parameter past the argument count splices nothing.
        let a = atoms("\\newcommand{\\q}[1]{#1#2}\\q{a}");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].nucleus, Field::Symbol('a'));
    }

    #[test]
    fn macro_optional_argument_defaults() {
        let def = "\\newcommand{\\rt}[2][2]{\\sqrt[#1]{#2}}";
        let a = atoms(&format!("{def}\\rt{{x}}"));
        let Field::Radical { degree, .. } = &a[0].nucleus else {
            panic!("expected radical, got {:?}", a[0].nucleus)
        };
        let deg = degree.as_ref().expect("default fills the degree");
        assert_eq!(deg.atoms().next().unwrap().nucleus, Field::Symbol('2'));
        let a = atoms(&format!("{def}\\rt[3]{{x}}"));
        let Field::Radical { degree, .. } = &a[0].nucleus else {
            panic!("expected radical, got {:?}", a[0].nucleus)
        };
        let deg = degree.as_ref().expect("bracket overrides the default");
        assert_eq!(deg.atoms().next().unwrap().nucleus, Field::Symbol('3'));
    }

    #[test]
    fn renewcommand_overrides_and_macros_shadow_vocabulary() {
        let a = atoms("\\newcommand{\\f}{x}\\renewcommand{\\f}{y}\\f");
        assert_eq!(a[0].nucleus, Field::Symbol('y'));
        let a = atoms("\\renewcommand{\\alpha}{\\beta}\\alpha");
        assert_eq!(a[0].nucleus, Field::Symbol('\u{03B2}'));
    }

    #[test]
    fn runaway_macros_hit_their_budgets() {
        // Self-recursion terminates, the cap leaving a literal tail.
        let list = parse("\\newcommand{\\a}{\\a}\\a");
        assert!(list
            .atoms()
            .any(|at| matches!(&at.nucleus, Field::Literal(t) if t == "\\a")));
        // A doubling bomb stays bounded instead of exploding.
        let bomb = "\\newcommand{\\a}{zzzzzzzzzz}\
                    \\newcommand{\\b}{\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a}\
                    \\newcommand{\\c}{\\b\\b\\b\\b\\b\\b\\b\\b\\b\\b}\
                    \\newcommand{\\d}{\\c\\c\\c\\c\\c\\c\\c\\c\\c\\c}\
                    \\newcommand{\\e}{\\d\\d\\d\\d\\d\\d\\d\\d\\d\\d}\\e";
        let list = parse(bomb);
        assert!(list.atoms().count() < 100_000);
        // The token ceiling stops a wide, shallow expansion; the numbers
        // sit above the 50k ceiling but below the expansion cap.
        let body = "z".repeat(300);
        let calls = "\\z".repeat(220);
        let list = parse(&format!("\\newcommand{{\\z}}{{{body}}}{calls}"));
        assert!(list.atoms().count() < 220 * 300);
        assert!(list
            .atoms()
            .any(|at| matches!(&at.nucleus, Field::Literal(t) if t == "\\z")));
    }

    #[test]
    fn malformed_definitions_degrade_to_literals() {
        for tex in [
            "\\newcommand",
            "\\newcommand{\\a}",
            "\\newcommand{a}{b}",
            "\\newcommand{\\a}[x]{b}",
            "\\newcommand{\\a}[12]{b}",
            "\\newcommand{\\a",
            "\\newcommand{\\a}{b",
            "\\renewcommand",
        ] {
            let _ = parse(tex);
        }
        let a = atoms("\\newcommand{a}{b}x");
        assert!(matches!(&a[0].nucleus, Field::Literal(_)));
        assert_eq!(a.last().unwrap().nucleus, Field::Symbol('x'));
    }

    #[test]
    fn expansions_stamp_the_call_site() {
        let src = "\\newcommand{\\R}{\\mathbb{R}}\\R^2";
        let a = atoms(src);
        assert_eq!(a.len(), 1);
        let call = src.rfind("\\R").unwrap();
        assert_eq!(a[0].nucleus_span, call..call + 2);
        assert_eq!(a[0].span, call..src.len());
        // Argument tokens keep their own source spans.
        let src = "\\newcommand{\\w}[1]{\\hat{#1}}\\w{x}";
        let a = atoms(src);
        let Field::Accent { base, .. } = &a[0].nucleus else {
            panic!("expected accent, got {:?}", a[0].nucleus)
        };
        let x = src.rfind('x').unwrap();
        assert_eq!(base.atoms().next().unwrap().span, x..x + 1);
    }

    #[test]
    fn broken_environments_degrade_to_literals() {
        let a = atoms("\\begin{pmatrix} a & b");
        assert!(
            matches!(&a[0].nucleus, Field::Literal(t) if t.contains("\\begin{pmatrix}")),
            "unterminated environment degrades whole, got {:?}",
            a[0].nucleus
        );
        let a = atoms("\\begin{foo} x \\end{foo} y");
        assert!(
            matches!(&a[0].nucleus, Field::Literal(t) if t.contains("foo")),
            "unknown environment degrades, got {:?}",
            a[0].nucleus
        );
        assert_eq!(a[1].nucleus, Field::Symbol('y'));
        let a = atoms("\\end{pmatrix} x");
        assert!(matches!(&a[0].nucleus, Field::Literal(_)));
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
        let _ = parse("\\begin");
        let _ = parse("\\begin{");
        let _ = parse("\\begin{pmatrix");
        let _ = parse("{\\begin{matrix} a}");
    }
}
