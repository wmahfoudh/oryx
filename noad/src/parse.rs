//! The parser: tokens to a math list.
//!
//! Recursive descent with no error path that aborts. Unknown commands
//! become literal atoms, stray closers and alignment markers degrade to
//! literals, and an unclosed group closes at the end of input. Hostile
//! input costs a fallback, never a panic.

use crate::mlist::{Atom, AtomClass, ColAlign, Field, Limits, MathList, Noad, TableGaps};

/// The symbol vocabulary: command name to codepoint and class, sorted by
/// name for binary search. Coverage grows by adding rows; the layout
/// tests pin every codepoint to a glyph in the STIX fixture.
pub(crate) const VOCABULARY: &[(&str, char, AtomClass)] = &[
    ("Alpha", '\u{0391}', AtomClass::Ord),
    ("Beta", '\u{0392}', AtomClass::Ord),
    ("Box", '\u{25A1}', AtomClass::Ord),
    ("Bumpeq", '\u{224E}', AtomClass::Rel),
    ("Cap", '\u{22D2}', AtomClass::Bin),
    ("Chi", '\u{03A7}', AtomClass::Ord),
    ("Cup", '\u{22D3}', AtomClass::Bin),
    ("Delta", '\u{0394}', AtomClass::Ord),
    ("Diamond", '\u{25CA}', AtomClass::Ord),
    ("Downarrow", '\u{21D3}', AtomClass::Rel),
    ("Epsilon", '\u{0395}', AtomClass::Ord),
    ("Eta", '\u{0397}', AtomClass::Ord),
    ("Finv", '\u{2132}', AtomClass::Ord),
    ("Game", '\u{2141}', AtomClass::Ord),
    ("Gamma", '\u{0393}', AtomClass::Ord),
    ("Im", '\u{2111}', AtomClass::Ord),
    ("Iota", '\u{0399}', AtomClass::Ord),
    ("Join", '\u{22C8}', AtomClass::Rel),
    ("Kappa", '\u{039A}', AtomClass::Ord),
    ("Lambda", '\u{039B}', AtomClass::Ord),
    ("Leftarrow", '\u{21D0}', AtomClass::Rel),
    ("Leftrightarrow", '\u{21D4}', AtomClass::Rel),
    ("Longleftarrow", '\u{27F8}', AtomClass::Rel),
    ("Longleftrightarrow", '\u{27FA}', AtomClass::Rel),
    ("Longrightarrow", '\u{27F9}', AtomClass::Rel),
    ("Lsh", '\u{21B0}', AtomClass::Rel),
    ("Mu", '\u{039C}', AtomClass::Ord),
    ("Nu", '\u{039D}', AtomClass::Ord),
    ("Omega", '\u{03A9}', AtomClass::Ord),
    ("Omicron", '\u{039F}', AtomClass::Ord),
    ("P", '\u{00B6}', AtomClass::Ord),
    ("Phi", '\u{03A6}', AtomClass::Ord),
    ("Pi", '\u{03A0}', AtomClass::Ord),
    ("Psi", '\u{03A8}', AtomClass::Ord),
    ("Re", '\u{211C}', AtomClass::Ord),
    ("Rho", '\u{03A1}', AtomClass::Ord),
    ("Rightarrow", '\u{21D2}', AtomClass::Rel),
    ("Rsh", '\u{21B1}', AtomClass::Rel),
    ("S", '\u{00A7}', AtomClass::Ord),
    ("Sigma", '\u{03A3}', AtomClass::Ord),
    ("Subset", '\u{22D0}', AtomClass::Rel),
    ("Supset", '\u{22D1}', AtomClass::Rel),
    ("Tau", '\u{03A4}', AtomClass::Ord),
    ("Theta", '\u{0398}', AtomClass::Ord),
    ("Uparrow", '\u{21D1}', AtomClass::Rel),
    ("Updownarrow", '\u{21D5}', AtomClass::Rel),
    ("Upsilon", '\u{03A5}', AtomClass::Ord),
    ("Vdash", '\u{22A9}', AtomClass::Rel),
    ("Vert", '\u{2016}', AtomClass::Ord),
    ("Vvdash", '\u{22AA}', AtomClass::Rel),
    ("Xi", '\u{039E}', AtomClass::Ord),
    ("Zeta", '\u{0396}', AtomClass::Ord),
    ("aleph", '\u{2135}', AtomClass::Ord),
    ("alpha", '\u{03B1}', AtomClass::Ord),
    ("amalg", '\u{2A3F}', AtomClass::Bin),
    ("angle", '\u{2220}', AtomClass::Ord),
    ("approx", '\u{2248}', AtomClass::Rel),
    ("approxeq", '\u{224A}', AtomClass::Rel),
    ("ast", '\u{2217}', AtomClass::Bin),
    ("asymp", '\u{224D}', AtomClass::Rel),
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
    ("bullet", '\u{2219}', AtomClass::Bin),
    ("bumpeq", '\u{224F}', AtomClass::Rel),
    ("cap", '\u{2229}', AtomClass::Bin),
    ("cdot", '\u{22C5}', AtomClass::Bin),
    ("cdots", '\u{22EF}', AtomClass::Ord),
    ("checkmark", '\u{2713}', AtomClass::Ord),
    ("chi", '\u{03C7}', AtomClass::Ord),
    ("circ", '\u{2218}', AtomClass::Bin),
    ("circeq", '\u{2257}', AtomClass::Rel),
    ("circlearrowleft", '\u{21BA}', AtomClass::Rel),
    ("circlearrowright", '\u{21BB}', AtomClass::Rel),
    ("circledast", '\u{229B}', AtomClass::Bin),
    ("circledcirc", '\u{229A}', AtomClass::Bin),
    ("circleddash", '\u{229D}', AtomClass::Bin),
    ("clubsuit", '\u{2663}', AtomClass::Ord),
    ("colon", '\u{003A}', AtomClass::Punct),
    ("coloneqq", '\u{2254}', AtomClass::Rel),
    ("cong", '\u{2245}', AtomClass::Rel),
    ("coprod", '\u{2210}', AtomClass::Op),
    ("cup", '\u{222A}', AtomClass::Bin),
    ("curlyeqprec", '\u{22DE}', AtomClass::Rel),
    ("curlyeqsucc", '\u{22DF}', AtomClass::Rel),
    ("curlyvee", '\u{22CE}', AtomClass::Bin),
    ("curlywedge", '\u{22CF}', AtomClass::Bin),
    ("curvearrowleft", '\u{21B6}', AtomClass::Rel),
    ("curvearrowright", '\u{21B7}', AtomClass::Rel),
    ("dag", '\u{2020}', AtomClass::Ord),
    ("dagger", '\u{2020}', AtomClass::Bin),
    ("daleth", '\u{2138}', AtomClass::Ord),
    ("dashleftarrow", '\u{21E0}', AtomClass::Rel),
    ("dashrightarrow", '\u{21E2}', AtomClass::Rel),
    ("dashv", '\u{22A3}', AtomClass::Rel),
    ("ddag", '\u{2021}', AtomClass::Ord),
    ("ddagger", '\u{2021}', AtomClass::Bin),
    ("ddots", '\u{22F1}', AtomClass::Ord),
    ("degree", '\u{00B0}', AtomClass::Ord),
    ("delta", '\u{03B4}', AtomClass::Ord),
    ("diamond", '\u{22C4}', AtomClass::Bin),
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
    ("downarrow", '\u{2193}', AtomClass::Rel),
    ("downdownarrows", '\u{21CA}', AtomClass::Rel),
    ("downharpoonleft", '\u{21C3}', AtomClass::Rel),
    ("downharpoonright", '\u{21C2}', AtomClass::Rel),
    ("ell", '\u{2113}', AtomClass::Ord),
    ("emptyset", '\u{2205}', AtomClass::Ord),
    ("epsilon", '\u{03F5}', AtomClass::Ord),
    ("eqcirc", '\u{2256}', AtomClass::Rel),
    ("eqqcolon", '\u{2255}', AtomClass::Rel),
    ("eqslantgtr", '\u{2A96}', AtomClass::Rel),
    ("eqslantless", '\u{2A95}', AtomClass::Rel),
    ("equiv", '\u{2261}', AtomClass::Rel),
    ("eta", '\u{03B7}', AtomClass::Ord),
    ("eth", '\u{00F0}', AtomClass::Ord),
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
    ("gimel", '\u{2137}', AtomClass::Ord),
    ("gnapprox", '\u{2A8A}', AtomClass::Rel),
    ("gneq", '\u{2A88}', AtomClass::Rel),
    ("gneqq", '\u{2269}', AtomClass::Rel),
    ("gnsim", '\u{22E7}', AtomClass::Rel),
    ("gtrapprox", '\u{2A86}', AtomClass::Rel),
    ("gtrdot", '\u{22D7}', AtomClass::Rel),
    ("gtreqless", '\u{22DB}', AtomClass::Rel),
    ("gtrless", '\u{2277}', AtomClass::Rel),
    ("gtrsim", '\u{2273}', AtomClass::Rel),
    ("hbar", '\u{210F}', AtomClass::Ord),
    ("heartsuit", '\u{2661}', AtomClass::Ord),
    ("hookleftarrow", '\u{21A9}', AtomClass::Rel),
    ("hookrightarrow", '\u{21AA}', AtomClass::Rel),
    ("hslash", '\u{210F}', AtomClass::Ord),
    ("iff", '\u{27FA}', AtomClass::Rel),
    ("iiint", '\u{222D}', AtomClass::Op),
    ("iint", '\u{222C}', AtomClass::Op),
    ("imath", '\u{1D6A4}', AtomClass::Ord),
    ("impliedby", '\u{27F8}', AtomClass::Rel),
    ("implies", '\u{27F9}', AtomClass::Rel),
    ("in", '\u{2208}', AtomClass::Rel),
    ("infty", '\u{221E}', AtomClass::Ord),
    ("int", '\u{222B}', AtomClass::Op),
    ("intercal", '\u{22BA}', AtomClass::Bin),
    ("iota", '\u{03B9}', AtomClass::Ord),
    ("jmath", '\u{1D6A5}', AtomClass::Ord),
    ("kappa", '\u{03BA}', AtomClass::Ord),
    ("lVert", '\u{2016}', AtomClass::Open),
    ("lambda", '\u{03BB}', AtomClass::Ord),
    ("land", '\u{2227}', AtomClass::Bin),
    ("langle", '\u{27E8}', AtomClass::Open),
    ("lbrace", '\u{007B}', AtomClass::Open),
    ("lbrack", '\u{005B}', AtomClass::Open),
    ("lceil", '\u{2308}', AtomClass::Open),
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
    ("leftthreetimes", '\u{22CB}', AtomClass::Bin),
    ("leq", '\u{2264}', AtomClass::Rel),
    ("leqq", '\u{2266}', AtomClass::Rel),
    ("leqslant", '\u{2A7D}', AtomClass::Rel),
    ("lessapprox", '\u{2A85}', AtomClass::Rel),
    ("lessdot", '\u{22D6}', AtomClass::Rel),
    ("lesseqgtr", '\u{22DA}', AtomClass::Rel),
    ("lessgtr", '\u{2276}', AtomClass::Rel),
    ("lesssim", '\u{2272}', AtomClass::Rel),
    ("lfloor", '\u{230A}', AtomClass::Open),
    ("lgroup", '\u{27EE}', AtomClass::Open),
    ("lhd", '\u{22B2}', AtomClass::Bin),
    ("ll", '\u{226A}', AtomClass::Rel),
    ("llcorner", '\u{231E}', AtomClass::Open),
    ("lll", '\u{22D8}', AtomClass::Rel),
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
    ("lrcorner", '\u{231F}', AtomClass::Close),
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
    ("oslash", '\u{2298}', AtomClass::Bin),
    ("otimes", '\u{2297}', AtomClass::Bin),
    ("owns", '\u{220B}', AtomClass::Rel),
    ("parallel", '\u{2225}', AtomClass::Rel),
    ("partial", '\u{2202}', AtomClass::Ord),
    ("perp", '\u{22A5}', AtomClass::Rel),
    ("phi", '\u{03D5}', AtomClass::Ord),
    ("pi", '\u{03C0}', AtomClass::Ord),
    ("pitchfork", '\u{22D4}', AtomClass::Rel),
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
    ("rVert", '\u{2016}', AtomClass::Close),
    ("rangle", '\u{27E9}', AtomClass::Close),
    ("rbrace", '\u{007D}', AtomClass::Close),
    ("rbrack", '\u{005D}', AtomClass::Close),
    ("rceil", '\u{2309}', AtomClass::Close),
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
    ("searrow", '\u{2198}', AtomClass::Rel),
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
    ("vert", '\u{007C}', AtomClass::Ord),
    ("wedge", '\u{2227}', AtomClass::Bin),
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
pub(crate) const DELIMITERS: &[(&str, char)] = &[
    ("Downarrow", '\u{21D3}'),
    ("Uparrow", '\u{21D1}'),
    ("Updownarrow", '\u{21D5}'),
    ("Vert", '\u{2016}'),
    ("backslash", '\\'),
    ("downarrow", '\u{2193}'),
    ("lVert", '\u{2016}'),
    ("langle", '\u{27E8}'),
    ("lbrace", '{'),
    ("lbrack", '['),
    ("lceil", '\u{2308}'),
    ("lfloor", '\u{230A}'),
    ("lgroup", '\u{27EE}'),
    ("llcorner", '\u{231E}'),
    ("lmoustache", '\u{23B0}'),
    ("lrcorner", '\u{231F}'),
    ("lvert", '|'),
    ("rVert", '\u{2016}'),
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
pub(crate) const ACCENTS: &[(&str, char, bool)] = &[
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
pub(crate) const OP_NAMES: &[(&str, bool)] = &[
    ("Pr", true),
    ("arccos", false),
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
        _ => return None,
    })
}

/// One letter-style command's codepoint remap into the Mathematical
/// Alphanumeric block, Letterlike Symbols holes included. A character
/// outside the command's alphabet stays itself.
fn map_alphabet(name: &str, c: char) -> char {
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
        let Noad::Atom(atom) = noad;
        restyle_field(&mut atom.nucleus, name);
        if let Some(s) = &mut atom.sup {
            restyle(s, name);
        }
        if let Some(s) = &mut atom.sub {
            restyle(s, name);
        }
    }
}

fn restyle_field(field: &mut Field, name: &str) {
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
        Field::Text(_) | Field::Literal(_) | Field::Kern(_) | Field::Empty => {}
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

/// The TeX demotion rule: a binary atom with no quantity on its left reads
/// as an ordinary symbol, so leading signs and doubled operators space as
/// signs. Applies per list; groups and scripts recurse through `list`.
fn demote_bins(items: &mut [Noad]) {
    let mut prev: Option<AtomClass> = None;
    for noad in items.iter_mut() {
        let Noad::Atom(atom) = noad;
        // Kerns are not atoms: demotion reads through them.
        if matches!(atom.nucleus, Field::Kern(_)) {
            continue;
        }
        if atom.class == AtomClass::Bin
            && !matches!(
                prev,
                Some(AtomClass::Ord) | Some(AtomClass::Close) | Some(AtomClass::Inner)
            )
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
        let mut items: Vec<Noad> = Vec::new();
        while let Some(tok) = self.peek() {
            let span = tok.span.clone();
            match &tok.kind {
                K::EndGroup => {
                    self.pos += 1;
                    if !top {
                        break;
                    }
                    items.push(Noad::Atom(literal_atom("}", span)));
                }
                K::Align => {
                    self.pos += 1;
                    items.push(Noad::Atom(literal_atom("&", span)));
                }
                K::Sup | K::Sub | K::Prime => {
                    // A script with nothing to hang on: TeX's implicit empty
                    // group becomes an empty-nucleus atom.
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
                    items.push(Noad::Atom(atom));
                }
                _ => {
                    if let Some(mut atom) = self.atom() {
                        self.scripts(&mut atom);
                        items.push(Noad::Atom(atom));
                    }
                }
            }
        }
        demote_bins(&mut items);
        MathList(items)
    }

    /// One scriptless atom from the stream: a character, a command, or a
    /// braced group. The caller has excluded every other token kind.
    /// Definitions and macro calls resolve here first, transparent to
    /// every construct that reads atoms; whenever the stream is nonempty
    /// the call consumes at least one token.
    fn atom(&mut self) -> Option<Atom> {
        use crate::token::TokenKind as K;
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
                        continue;
                    }
                    return Some(literal_atom(format!("\\{name}"), tok.span));
                }
            }
            break tok;
        };
        match tok.kind {
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
                "frac" => self.fraction(tok.span, true),
                "binom" => self.binom(tok.span),
                "sqrt" => self.radical(tok.span),
                "left" => self.left_right(tok.span),
                "right" => {
                    // A stray closer: its delimiter goes with it, the pair
                    // degrades to a literal.
                    let _ = self.delimiter();
                    literal_atom("\\right", tok.span)
                }
                "text" | "mathrm" => self.text(tok.span, &name),
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
                "mathbb" | "mathbf" | "mathcal" | "mathfrak" | "mathit" | "mathsf" | "mathtt" => {
                    self.styled(tok.span, &name)
                }
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
            _ => None,
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

    /// `\frac{num}{den}`; the argument reader accepts single tokens the
    /// way TeX does, so `\frac12` works.
    fn fraction(&mut self, start: std::ops::Range<usize>, bar: bool) -> Atom {
        let numerator = self.script_operand();
        let denominator = self.script_operand();
        self.construct(
            start,
            Field::Fraction {
                numerator,
                denominator,
                bar,
            },
            AtomClass::Inner,
        )
    }

    /// `\binom{n}{k}`: a barless stack inside stretched parentheses.
    fn binom(&mut self, start: std::ops::Range<usize>) -> Atom {
        let numerator = self.script_operand();
        let denominator = self.script_operand();
        let end = self.consumed_end(start.end);
        let span = start.start..end;
        let stack = Atom {
            class: AtomClass::Inner,
            nucleus: Field::Fraction {
                numerator,
                denominator,
                bar: false,
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
                open: Some('('),
                close: Some(')'),
                body: MathList(vec![Noad::Atom(stack)]),
            },
            sup: None,
            sub: None,
            limits: Limits::default(),
            span: span.clone(),
            nucleus_span: span,
        }
    }

    /// A letter-style command: the operand parses normally, then its
    /// symbols remap into the command's alphabet. A single restyled atom
    /// keeps its own class; a longer operand wraps as a group.
    fn styled(&mut self, start: std::ops::Range<usize>, name: &str) -> Atom {
        let mut operand = self.script_operand();
        restyle(&mut operand, name);
        let end = self.consumed_end(start.end);
        let span = start.start..end;
        if operand.0.len() == 1 {
            let Noad::Atom(mut atom) = operand.0.pop().expect("one noad");
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
    /// spaces and nested braces included, which the tokenizer's spans
    /// recover from the source. Without a group the command degrades to
    /// a literal under its own name.
    fn text(&mut self, start: std::ops::Range<usize>, name: &str) -> Atom {
        use crate::token::TokenKind as K;
        if !matches!(self.peek().map(|t| &t.kind), Some(K::BeginGroup)) {
            return literal_atom(format!("\\{name}"), start);
        }
        let open = self.next().expect("peeked");
        let content_start = open.span.end;
        let mut content_end = content_start;
        let mut depth = 1usize;
        while let Some(tok) = self.next() {
            match tok.kind {
                K::BeginGroup => depth += 1,
                K::EndGroup => {
                    depth -= 1;
                    if depth == 0 {
                        content_end = tok.span.start;
                        break;
                    }
                }
                _ => {}
            }
            content_end = tok.span.end;
        }
        let text = self.src.get(content_start..content_end).unwrap_or("");
        self.construct(start, Field::Text(text.to_string()), AtomClass::Ord)
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
        let mut cell: Vec<Noad> = Vec::new();
        loop {
            let Some(tok) = self.peek() else {
                return (rows, false);
            };
            let span = tok.span.clone();
            match &tok.kind {
                K::Align => {
                    self.pos += 1;
                    demote_bins(&mut cell);
                    row.push(MathList(std::mem::take(&mut cell)));
                }
                K::Command(name) if name == "\\" => {
                    self.pos += 1;
                    demote_bins(&mut cell);
                    row.push(MathList(std::mem::take(&mut cell)));
                    rows.push(std::mem::take(&mut row));
                }
                K::Command(name) if name == "end" => {
                    self.pos += 1;
                    let _ = self.env_name();
                    demote_bins(&mut cell);
                    row.push(MathList(std::mem::take(&mut cell)));
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
                K::Sup | K::Sub | K::Prime => {
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
                    cell.push(Noad::Atom(atom));
                }
                _ => {
                    if let Some(mut atom) = self.atom() {
                        self.scripts(&mut atom);
                        cell.push(Noad::Atom(atom));
                    } else {
                        self.pos += 1;
                    }
                }
            }
        }
    }

    /// `\sqrt{x}` with the optional `[degree]`.
    fn radical(&mut self, start: std::ops::Range<usize>) -> Atom {
        use crate::token::TokenKind as K;
        let degree = if matches!(self.peek().map(|t| &t.kind), Some(K::Char('['))) {
            self.pos += 1;
            Some(self.list_until_char(']'))
        } else {
            None
        };
        let radicand = self.script_operand();
        self.construct(start, Field::Radical { radicand, degree }, AtomClass::Ord)
    }

    /// Elements up to a closing character, consumed; end of input closes.
    fn list_until_char(&mut self, closer: char) -> MathList {
        use crate::token::TokenKind as K;
        let mut items: Vec<Noad> = Vec::new();
        while let Some(tok) = self.peek() {
            if matches!(&tok.kind, K::Char(c) if *c == closer) {
                self.pos += 1;
                break;
            }
            if matches!(tok.kind, K::EndGroup) {
                break;
            }
            if let Some(mut atom) = self.atom() {
                self.scripts(&mut atom);
                items.push(Noad::Atom(atom));
            } else {
                self.pos += 1;
            }
        }
        demote_bins(&mut items);
        MathList(items)
    }

    /// `\left⟨delim⟩ ... \right⟨delim⟩`. A missing `\right` fails open at
    /// the end of input or at the enclosing group's closer, which stays
    /// for the group to consume.
    fn left_right(&mut self, start: std::ops::Range<usize>) -> Atom {
        use crate::token::TokenKind as K;
        let open = self.delimiter();
        let mut items: Vec<Noad> = Vec::new();
        let mut close = None;
        while let Some(tok) = self.peek() {
            match &tok.kind {
                K::Command(name) if name == "right" => {
                    self.pos += 1;
                    close = self.delimiter();
                    break;
                }
                K::EndGroup => break,
                _ => {
                    if let Some(mut atom) = self.atom() {
                        self.scripts(&mut atom);
                        items.push(Noad::Atom(atom));
                    } else {
                        self.pos += 1;
                    }
                }
            }
        }
        demote_bins(&mut items);
        self.construct(
            start,
            Field::LeftRight {
                open,
                close,
                body: MathList(items),
            },
            AtomClass::Inner,
        )
    }

    /// One delimiter token after `\left` or `\right`: a character, `.` for
    /// none, or a delimiter command. Anything else answers none and stays.
    fn delimiter(&mut self) -> Option<char> {
        use crate::token::TokenKind as K;
        let resolved = match self.peek().map(|t| &t.kind) {
            Some(K::Char('.')) => Some(None),
            Some(K::Char(c)) => Some(Some(*c)),
            Some(K::Command(name)) => DELIMITERS
                .binary_search_by(|row| row.0.cmp(name.as_str()))
                .ok()
                .map(|i| Some(DELIMITERS[i].1)),
            _ => None,
        };
        match resolved {
            Some(delim) => {
                self.pos += 1;
                delim
            }
            None => None,
        }
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
            K::Char(_) | K::Command(_) => {
                let atom = self.atom().expect("peeked");
                MathList(vec![Noad::Atom(atom)])
            }
            K::Sup | K::Sub | K::Prime | K::Align | K::EndGroup => {
                // ^ with no legal operand: degrade the marker itself.
                let text = match tok.kind {
                    K::Sup => "^",
                    K::Sub => "_",
                    K::Prime => "'",
                    K::Align => "&",
                    _ => "}",
                };
                self.pos += 1;
                MathList(vec![Noad::Atom(literal_atom(text, span))])
            }
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
    fn vocabulary_is_sorted_and_duplicate_free() {
        for pair in VOCABULARY.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
    }

    #[test]
    fn commands_resolve_symbol_and_class() {
        let a = atoms("\\alpha\\pm\\leq\\sum");
        assert_eq!(a.len(), 4);
        assert_eq!(a[0].nucleus, Field::Symbol('\u{03B1}'));
        assert_eq!(a[0].class, AtomClass::Ord);
        assert_eq!(a[1].nucleus, Field::Symbol('\u{00B1}'));
        assert_eq!(a[1].class, AtomClass::Bin);
        assert_eq!(a[2].class, AtomClass::Rel);
        assert_eq!(a[3].nucleus, Field::Symbol('\u{2211}'));
        assert_eq!(a[3].class, AtomClass::Op);
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
        // No group degrades quietly.
        let a = atoms("\\text x");
        assert_eq!(a[0].nucleus, Field::Literal("\\text".into()));
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
    fn mathrm_and_operatorname_render_upright() {
        let a = atoms("\\mathrm{d}x");
        assert_eq!(a[0].nucleus, Field::Text("d".into()));
        assert_eq!(a[0].class, AtomClass::Ord);
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
        // A missing group degrades with the command's own name.
        let a = atoms("\\mathrm x");
        assert_eq!(a[0].nucleus, Field::Literal("\\mathrm".into()));
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
