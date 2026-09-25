//! Differential probes of depth and size: nested generic applications,
//! wide literal unions, many declarations, recursive and deeply nested
//! conditional types, long function bodies (return chains, narrowing
//! chains, conditional writes, wide object literals, `&&` chains), loops
//! over locals that feed each other, overload lists, and call chains. The
//! passing tests hold sizes the lane answers quickly; each ignored test
//! holds the smallest size found to crash, hang or fail, with the sizes
//! measured beside it.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on the exact
//! fixture with `tsc --ignoreConfig --declaration --emitDeclarationOnly
//! --strict --noErrorTruncation` under each `strictNullChecks` ×
//! `noImplicitAny` setting: `declare const p: <probe>; const s: never =
//! p;` (a function row reads `ReturnType<typeof f>`) read off the TS2322
//! message; the checker answers every row at once. A row with two answers
//! gives the `strictNullChecks` answer then the answer with it off. An
//! overdue row fails after the harness's row deadline, so a hanging ignored
//! test takes that deadline for each setting.

use super::differential_harness_tests::{Matrix, Read};

/// A 40-deep generic application, a 100-member literal union, a recursive tuple
/// builder, a 40-branch conditional and 200 interfaces in a cycle.
const DEEP_SHAPES: &str = r##"interface Box<T> { v: T }
type D = Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<1>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>;
type W = "m0" | "m1" | "m2" | "m3" | "m4" | "m5" | "m6" | "m7" | "m8" | "m9" | "m10" | "m11" | "m12" | "m13" | "m14" | "m15" | "m16" | "m17" | "m18" | "m19" | "m20" | "m21" | "m22" | "m23" | "m24" | "m25" | "m26" | "m27" | "m28" | "m29" | "m30" | "m31" | "m32" | "m33" | "m34" | "m35" | "m36" | "m37" | "m38" | "m39" | "m40" | "m41" | "m42" | "m43" | "m44" | "m45" | "m46" | "m47" | "m48" | "m49" | "m50" | "m51" | "m52" | "m53" | "m54" | "m55" | "m56" | "m57" | "m58" | "m59" | "m60" | "m61" | "m62" | "m63" | "m64" | "m65" | "m66" | "m67" | "m68" | "m69" | "m70" | "m71" | "m72" | "m73" | "m74" | "m75" | "m76" | "m77" | "m78" | "m79" | "m80" | "m81" | "m82" | "m83" | "m84" | "m85" | "m86" | "m87" | "m88" | "m89" | "m90" | "m91" | "m92" | "m93" | "m94" | "m95" | "m96" | "m97" | "m98" | "m99";
type Build<N extends number, A extends unknown[] = []> = A["length"] extends N ? A : Build<N, [...A, 0]>;
type Pick1<T> = T extends 0 ? "c0" : T extends 1 ? "c1" : T extends 2 ? "c2" : T extends 3 ? "c3" : T extends 4 ? "c4" : T extends 5 ? "c5" : T extends 6 ? "c6" : T extends 7 ? "c7" : T extends 8 ? "c8" : T extends 9 ? "c9" : T extends 10 ? "c10" : T extends 11 ? "c11" : T extends 12 ? "c12" : T extends 13 ? "c13" : T extends 14 ? "c14" : T extends 15 ? "c15" : T extends 16 ? "c16" : T extends 17 ? "c17" : T extends 18 ? "c18" : T extends 19 ? "c19" : T extends 20 ? "c20" : T extends 21 ? "c21" : T extends 22 ? "c22" : T extends 23 ? "c23" : T extends 24 ? "c24" : T extends 25 ? "c25" : T extends 26 ? "c26" : T extends 27 ? "c27" : T extends 28 ? "c28" : T extends 29 ? "c29" : T extends 30 ? "c30" : T extends 31 ? "c31" : T extends 32 ? "c32" : T extends 33 ? "c33" : T extends 34 ? "c34" : T extends 35 ? "c35" : T extends 36 ? "c36" : T extends 37 ? "c37" : T extends 38 ? "c38" : T extends 39 ? "c39" : "none";
interface I0 { p0: 0; next: I1 }
interface I1 { p1: 1; next: I2 }
interface I2 { p2: 2; next: I3 }
interface I3 { p3: 3; next: I4 }
interface I4 { p4: 4; next: I5 }
interface I5 { p5: 5; next: I6 }
interface I6 { p6: 6; next: I7 }
interface I7 { p7: 7; next: I8 }
interface I8 { p8: 8; next: I9 }
interface I9 { p9: 9; next: I10 }
interface I10 { p10: 10; next: I11 }
interface I11 { p11: 11; next: I12 }
interface I12 { p12: 12; next: I13 }
interface I13 { p13: 13; next: I14 }
interface I14 { p14: 14; next: I15 }
interface I15 { p15: 15; next: I16 }
interface I16 { p16: 16; next: I17 }
interface I17 { p17: 17; next: I18 }
interface I18 { p18: 18; next: I19 }
interface I19 { p19: 19; next: I20 }
interface I20 { p20: 20; next: I21 }
interface I21 { p21: 21; next: I22 }
interface I22 { p22: 22; next: I23 }
interface I23 { p23: 23; next: I24 }
interface I24 { p24: 24; next: I25 }
interface I25 { p25: 25; next: I26 }
interface I26 { p26: 26; next: I27 }
interface I27 { p27: 27; next: I28 }
interface I28 { p28: 28; next: I29 }
interface I29 { p29: 29; next: I30 }
interface I30 { p30: 30; next: I31 }
interface I31 { p31: 31; next: I32 }
interface I32 { p32: 32; next: I33 }
interface I33 { p33: 33; next: I34 }
interface I34 { p34: 34; next: I35 }
interface I35 { p35: 35; next: I36 }
interface I36 { p36: 36; next: I37 }
interface I37 { p37: 37; next: I38 }
interface I38 { p38: 38; next: I39 }
interface I39 { p39: 39; next: I40 }
interface I40 { p40: 40; next: I41 }
interface I41 { p41: 41; next: I42 }
interface I42 { p42: 42; next: I43 }
interface I43 { p43: 43; next: I44 }
interface I44 { p44: 44; next: I45 }
interface I45 { p45: 45; next: I46 }
interface I46 { p46: 46; next: I47 }
interface I47 { p47: 47; next: I48 }
interface I48 { p48: 48; next: I49 }
interface I49 { p49: 49; next: I50 }
interface I50 { p50: 50; next: I51 }
interface I51 { p51: 51; next: I52 }
interface I52 { p52: 52; next: I53 }
interface I53 { p53: 53; next: I54 }
interface I54 { p54: 54; next: I55 }
interface I55 { p55: 55; next: I56 }
interface I56 { p56: 56; next: I57 }
interface I57 { p57: 57; next: I58 }
interface I58 { p58: 58; next: I59 }
interface I59 { p59: 59; next: I60 }
interface I60 { p60: 60; next: I61 }
interface I61 { p61: 61; next: I62 }
interface I62 { p62: 62; next: I63 }
interface I63 { p63: 63; next: I64 }
interface I64 { p64: 64; next: I65 }
interface I65 { p65: 65; next: I66 }
interface I66 { p66: 66; next: I67 }
interface I67 { p67: 67; next: I68 }
interface I68 { p68: 68; next: I69 }
interface I69 { p69: 69; next: I70 }
interface I70 { p70: 70; next: I71 }
interface I71 { p71: 71; next: I72 }
interface I72 { p72: 72; next: I73 }
interface I73 { p73: 73; next: I74 }
interface I74 { p74: 74; next: I75 }
interface I75 { p75: 75; next: I76 }
interface I76 { p76: 76; next: I77 }
interface I77 { p77: 77; next: I78 }
interface I78 { p78: 78; next: I79 }
interface I79 { p79: 79; next: I80 }
interface I80 { p80: 80; next: I81 }
interface I81 { p81: 81; next: I82 }
interface I82 { p82: 82; next: I83 }
interface I83 { p83: 83; next: I84 }
interface I84 { p84: 84; next: I85 }
interface I85 { p85: 85; next: I86 }
interface I86 { p86: 86; next: I87 }
interface I87 { p87: 87; next: I88 }
interface I88 { p88: 88; next: I89 }
interface I89 { p89: 89; next: I90 }
interface I90 { p90: 90; next: I91 }
interface I91 { p91: 91; next: I92 }
interface I92 { p92: 92; next: I93 }
interface I93 { p93: 93; next: I94 }
interface I94 { p94: 94; next: I95 }
interface I95 { p95: 95; next: I96 }
interface I96 { p96: 96; next: I97 }
interface I97 { p97: 97; next: I98 }
interface I98 { p98: 98; next: I99 }
interface I99 { p99: 99; next: I100 }
interface I100 { p100: 100; next: I101 }
interface I101 { p101: 101; next: I102 }
interface I102 { p102: 102; next: I103 }
interface I103 { p103: 103; next: I104 }
interface I104 { p104: 104; next: I105 }
interface I105 { p105: 105; next: I106 }
interface I106 { p106: 106; next: I107 }
interface I107 { p107: 107; next: I108 }
interface I108 { p108: 108; next: I109 }
interface I109 { p109: 109; next: I110 }
interface I110 { p110: 110; next: I111 }
interface I111 { p111: 111; next: I112 }
interface I112 { p112: 112; next: I113 }
interface I113 { p113: 113; next: I114 }
interface I114 { p114: 114; next: I115 }
interface I115 { p115: 115; next: I116 }
interface I116 { p116: 116; next: I117 }
interface I117 { p117: 117; next: I118 }
interface I118 { p118: 118; next: I119 }
interface I119 { p119: 119; next: I120 }
interface I120 { p120: 120; next: I121 }
interface I121 { p121: 121; next: I122 }
interface I122 { p122: 122; next: I123 }
interface I123 { p123: 123; next: I124 }
interface I124 { p124: 124; next: I125 }
interface I125 { p125: 125; next: I126 }
interface I126 { p126: 126; next: I127 }
interface I127 { p127: 127; next: I128 }
interface I128 { p128: 128; next: I129 }
interface I129 { p129: 129; next: I130 }
interface I130 { p130: 130; next: I131 }
interface I131 { p131: 131; next: I132 }
interface I132 { p132: 132; next: I133 }
interface I133 { p133: 133; next: I134 }
interface I134 { p134: 134; next: I135 }
interface I135 { p135: 135; next: I136 }
interface I136 { p136: 136; next: I137 }
interface I137 { p137: 137; next: I138 }
interface I138 { p138: 138; next: I139 }
interface I139 { p139: 139; next: I140 }
interface I140 { p140: 140; next: I141 }
interface I141 { p141: 141; next: I142 }
interface I142 { p142: 142; next: I143 }
interface I143 { p143: 143; next: I144 }
interface I144 { p144: 144; next: I145 }
interface I145 { p145: 145; next: I146 }
interface I146 { p146: 146; next: I147 }
interface I147 { p147: 147; next: I148 }
interface I148 { p148: 148; next: I149 }
interface I149 { p149: 149; next: I150 }
interface I150 { p150: 150; next: I151 }
interface I151 { p151: 151; next: I152 }
interface I152 { p152: 152; next: I153 }
interface I153 { p153: 153; next: I154 }
interface I154 { p154: 154; next: I155 }
interface I155 { p155: 155; next: I156 }
interface I156 { p156: 156; next: I157 }
interface I157 { p157: 157; next: I158 }
interface I158 { p158: 158; next: I159 }
interface I159 { p159: 159; next: I160 }
interface I160 { p160: 160; next: I161 }
interface I161 { p161: 161; next: I162 }
interface I162 { p162: 162; next: I163 }
interface I163 { p163: 163; next: I164 }
interface I164 { p164: 164; next: I165 }
interface I165 { p165: 165; next: I166 }
interface I166 { p166: 166; next: I167 }
interface I167 { p167: 167; next: I168 }
interface I168 { p168: 168; next: I169 }
interface I169 { p169: 169; next: I170 }
interface I170 { p170: 170; next: I171 }
interface I171 { p171: 171; next: I172 }
interface I172 { p172: 172; next: I173 }
interface I173 { p173: 173; next: I174 }
interface I174 { p174: 174; next: I175 }
interface I175 { p175: 175; next: I176 }
interface I176 { p176: 176; next: I177 }
interface I177 { p177: 177; next: I178 }
interface I178 { p178: 178; next: I179 }
interface I179 { p179: 179; next: I180 }
interface I180 { p180: 180; next: I181 }
interface I181 { p181: 181; next: I182 }
interface I182 { p182: 182; next: I183 }
interface I183 { p183: 183; next: I184 }
interface I184 { p184: 184; next: I185 }
interface I185 { p185: 185; next: I186 }
interface I186 { p186: 186; next: I187 }
interface I187 { p187: 187; next: I188 }
interface I188 { p188: 188; next: I189 }
interface I189 { p189: 189; next: I190 }
interface I190 { p190: 190; next: I191 }
interface I191 { p191: 191; next: I192 }
interface I192 { p192: 192; next: I193 }
interface I193 { p193: 193; next: I194 }
interface I194 { p194: 194; next: I195 }
interface I195 { p195: 195; next: I196 }
interface I196 { p196: 196; next: I197 }
interface I197 { p197: 197; next: I198 }
interface I198 { p198: 198; next: I199 }
interface I199 { p199: 199; next: I0 }
"##;

/// Forty member reads through a 40-deep `Box<…>` application, its relation to
/// the same shape over `number`, membership and `Exclude` over a 100-member
/// union, a recursive tuple built to length 40, a 40-branch conditional chain,
/// and reads through 200 interfaces that reference each other.
#[test]
fn deep_and_wide_types_answer_as_the_checker_answers() {
    let matrix = Matrix::new(DEEP_SHAPES);
    let failures = matrix.types(&[
        ("D['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']", "1"),
        ("[D] extends [Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<number>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>] ? 1 : 2", "1"),
        ("[\"m99\"] extends [W] ? 1 : 2", "1"),
        ("[\"zz\"] extends [W] ? 1 : 2", "2"),
        ("[Exclude<W, \"m0\">] extends [W] ? 1 : 2", "1"),
        ("Build<40>[\"length\"]", "40"),
        ("Pick1<39>", "\"c39\""),
        ("Pick1<\"x\">", "\"none\""),
        ("I0['next']['next']['p2']", "2"),
        ("I199['next']['p0']", "0"),
        ("[I0] extends [I1] ? 1 : 2", "2"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Functions with 100 returning guards, 49 narrowing guards, 50 conditional
/// writes, a 400-property object literal and a 20-operand `&&` chain.
const LONG_BODIES: &str = r##"declare function c0(): boolean;
export function chain(x: number) {
  if (x === 0) return "r0" as const;
  if (x === 1) return "r1" as const;
  if (x === 2) return "r2" as const;
  if (x === 3) return "r3" as const;
  if (x === 4) return "r4" as const;
  if (x === 5) return "r5" as const;
  if (x === 6) return "r6" as const;
  if (x === 7) return "r7" as const;
  if (x === 8) return "r8" as const;
  if (x === 9) return "r9" as const;
  if (x === 10) return "r10" as const;
  if (x === 11) return "r11" as const;
  if (x === 12) return "r12" as const;
  if (x === 13) return "r13" as const;
  if (x === 14) return "r14" as const;
  if (x === 15) return "r15" as const;
  if (x === 16) return "r16" as const;
  if (x === 17) return "r17" as const;
  if (x === 18) return "r18" as const;
  if (x === 19) return "r19" as const;
  if (x === 20) return "r20" as const;
  if (x === 21) return "r21" as const;
  if (x === 22) return "r22" as const;
  if (x === 23) return "r23" as const;
  if (x === 24) return "r24" as const;
  if (x === 25) return "r25" as const;
  if (x === 26) return "r26" as const;
  if (x === 27) return "r27" as const;
  if (x === 28) return "r28" as const;
  if (x === 29) return "r29" as const;
  if (x === 30) return "r30" as const;
  if (x === 31) return "r31" as const;
  if (x === 32) return "r32" as const;
  if (x === 33) return "r33" as const;
  if (x === 34) return "r34" as const;
  if (x === 35) return "r35" as const;
  if (x === 36) return "r36" as const;
  if (x === 37) return "r37" as const;
  if (x === 38) return "r38" as const;
  if (x === 39) return "r39" as const;
  if (x === 40) return "r40" as const;
  if (x === 41) return "r41" as const;
  if (x === 42) return "r42" as const;
  if (x === 43) return "r43" as const;
  if (x === 44) return "r44" as const;
  if (x === 45) return "r45" as const;
  if (x === 46) return "r46" as const;
  if (x === 47) return "r47" as const;
  if (x === 48) return "r48" as const;
  if (x === 49) return "r49" as const;
  if (x === 50) return "r50" as const;
  if (x === 51) return "r51" as const;
  if (x === 52) return "r52" as const;
  if (x === 53) return "r53" as const;
  if (x === 54) return "r54" as const;
  if (x === 55) return "r55" as const;
  if (x === 56) return "r56" as const;
  if (x === 57) return "r57" as const;
  if (x === 58) return "r58" as const;
  if (x === 59) return "r59" as const;
  if (x === 60) return "r60" as const;
  if (x === 61) return "r61" as const;
  if (x === 62) return "r62" as const;
  if (x === 63) return "r63" as const;
  if (x === 64) return "r64" as const;
  if (x === 65) return "r65" as const;
  if (x === 66) return "r66" as const;
  if (x === 67) return "r67" as const;
  if (x === 68) return "r68" as const;
  if (x === 69) return "r69" as const;
  if (x === 70) return "r70" as const;
  if (x === 71) return "r71" as const;
  if (x === 72) return "r72" as const;
  if (x === 73) return "r73" as const;
  if (x === 74) return "r74" as const;
  if (x === 75) return "r75" as const;
  if (x === 76) return "r76" as const;
  if (x === 77) return "r77" as const;
  if (x === 78) return "r78" as const;
  if (x === 79) return "r79" as const;
  if (x === 80) return "r80" as const;
  if (x === 81) return "r81" as const;
  if (x === 82) return "r82" as const;
  if (x === 83) return "r83" as const;
  if (x === 84) return "r84" as const;
  if (x === 85) return "r85" as const;
  if (x === 86) return "r86" as const;
  if (x === 87) return "r87" as const;
  if (x === 88) return "r88" as const;
  if (x === 89) return "r89" as const;
  if (x === 90) return "r90" as const;
  if (x === 91) return "r91" as const;
  if (x === 92) return "r92" as const;
  if (x === 93) return "r93" as const;
  if (x === 94) return "r94" as const;
  if (x === 95) return "r95" as const;
  if (x === 96) return "r96" as const;
  if (x === 97) return "r97" as const;
  if (x === 98) return "r98" as const;
  if (x === 99) return "r99" as const;
  return "none" as const;
}
type K = "k0" | "k1" | "k2" | "k3" | "k4" | "k5" | "k6" | "k7" | "k8" | "k9" | "k10" | "k11" | "k12" | "k13" | "k14" | "k15" | "k16" | "k17" | "k18" | "k19" | "k20" | "k21" | "k22" | "k23" | "k24" | "k25" | "k26" | "k27" | "k28" | "k29" | "k30" | "k31" | "k32" | "k33" | "k34" | "k35" | "k36" | "k37" | "k38" | "k39" | "k40" | "k41" | "k42" | "k43" | "k44" | "k45" | "k46" | "k47" | "k48" | "k49";
export function last(x: K) {
  if (x === "k0") throw 0;
  if (x === "k1") throw 0;
  if (x === "k2") throw 0;
  if (x === "k3") throw 0;
  if (x === "k4") throw 0;
  if (x === "k5") throw 0;
  if (x === "k6") throw 0;
  if (x === "k7") throw 0;
  if (x === "k8") throw 0;
  if (x === "k9") throw 0;
  if (x === "k10") throw 0;
  if (x === "k11") throw 0;
  if (x === "k12") throw 0;
  if (x === "k13") throw 0;
  if (x === "k14") throw 0;
  if (x === "k15") throw 0;
  if (x === "k16") throw 0;
  if (x === "k17") throw 0;
  if (x === "k18") throw 0;
  if (x === "k19") throw 0;
  if (x === "k20") throw 0;
  if (x === "k21") throw 0;
  if (x === "k22") throw 0;
  if (x === "k23") throw 0;
  if (x === "k24") throw 0;
  if (x === "k25") throw 0;
  if (x === "k26") throw 0;
  if (x === "k27") throw 0;
  if (x === "k28") throw 0;
  if (x === "k29") throw 0;
  if (x === "k30") throw 0;
  if (x === "k31") throw 0;
  if (x === "k32") throw 0;
  if (x === "k33") throw 0;
  if (x === "k34") throw 0;
  if (x === "k35") throw 0;
  if (x === "k36") throw 0;
  if (x === "k37") throw 0;
  if (x === "k38") throw 0;
  if (x === "k39") throw 0;
  if (x === "k40") throw 0;
  if (x === "k41") throw 0;
  if (x === "k42") throw 0;
  if (x === "k43") throw 0;
  if (x === "k44") throw 0;
  if (x === "k45") throw 0;
  if (x === "k46") throw 0;
  if (x === "k47") throw 0;
  if (x === "k48") throw 0;
  return x;
}
export function acc() {
  let v: number | string = "s";
  if (c0()) v = 0;
  if (c0()) v = 1;
  if (c0()) v = 2;
  if (c0()) v = 3;
  if (c0()) v = 4;
  if (c0()) v = 5;
  if (c0()) v = 6;
  if (c0()) v = 7;
  if (c0()) v = 8;
  if (c0()) v = 9;
  if (c0()) v = 10;
  if (c0()) v = 11;
  if (c0()) v = 12;
  if (c0()) v = 13;
  if (c0()) v = 14;
  if (c0()) v = 15;
  if (c0()) v = 16;
  if (c0()) v = 17;
  if (c0()) v = 18;
  if (c0()) v = 19;
  if (c0()) v = 20;
  if (c0()) v = 21;
  if (c0()) v = 22;
  if (c0()) v = 23;
  if (c0()) v = 24;
  if (c0()) v = 25;
  if (c0()) v = 26;
  if (c0()) v = 27;
  if (c0()) v = 28;
  if (c0()) v = 29;
  if (c0()) v = 30;
  if (c0()) v = 31;
  if (c0()) v = 32;
  if (c0()) v = 33;
  if (c0()) v = 34;
  if (c0()) v = 35;
  if (c0()) v = 36;
  if (c0()) v = 37;
  if (c0()) v = 38;
  if (c0()) v = 39;
  if (c0()) v = 40;
  if (c0()) v = 41;
  if (c0()) v = 42;
  if (c0()) v = 43;
  if (c0()) v = 44;
  if (c0()) v = 45;
  if (c0()) v = 46;
  if (c0()) v = 47;
  if (c0()) v = 48;
  if (c0()) v = 49;
  return v;
}
export function obj() { return { p0: 0, p1: 1, p2: 2, p3: 3, p4: 4, p5: 5, p6: 6, p7: 7, p8: 8, p9: 9, p10: 10, p11: 11, p12: 12, p13: 13, p14: 14, p15: 15, p16: 16, p17: 17, p18: 18, p19: 19, p20: 20, p21: 21, p22: 22, p23: 23, p24: 24, p25: 25, p26: 26, p27: 27, p28: 28, p29: 29, p30: 30, p31: 31, p32: 32, p33: 33, p34: 34, p35: 35, p36: 36, p37: 37, p38: 38, p39: 39, p40: 40, p41: 41, p42: 42, p43: 43, p44: 44, p45: 45, p46: 46, p47: 47, p48: 48, p49: 49, p50: 50, p51: 51, p52: 52, p53: 53, p54: 54, p55: 55, p56: 56, p57: 57, p58: 58, p59: 59, p60: 60, p61: 61, p62: 62, p63: 63, p64: 64, p65: 65, p66: 66, p67: 67, p68: 68, p69: 69, p70: 70, p71: 71, p72: 72, p73: 73, p74: 74, p75: 75, p76: 76, p77: 77, p78: 78, p79: 79, p80: 80, p81: 81, p82: 82, p83: 83, p84: 84, p85: 85, p86: 86, p87: 87, p88: 88, p89: 89, p90: 90, p91: 91, p92: 92, p93: 93, p94: 94, p95: 95, p96: 96, p97: 97, p98: 98, p99: 99, p100: 100, p101: 101, p102: 102, p103: 103, p104: 104, p105: 105, p106: 106, p107: 107, p108: 108, p109: 109, p110: 110, p111: 111, p112: 112, p113: 113, p114: 114, p115: 115, p116: 116, p117: 117, p118: 118, p119: 119, p120: 120, p121: 121, p122: 122, p123: 123, p124: 124, p125: 125, p126: 126, p127: 127, p128: 128, p129: 129, p130: 130, p131: 131, p132: 132, p133: 133, p134: 134, p135: 135, p136: 136, p137: 137, p138: 138, p139: 139, p140: 140, p141: 141, p142: 142, p143: 143, p144: 144, p145: 145, p146: 146, p147: 147, p148: 148, p149: 149, p150: 150, p151: 151, p152: 152, p153: 153, p154: 154, p155: 155, p156: 156, p157: 157, p158: 158, p159: 159, p160: 160, p161: 161, p162: 162, p163: 163, p164: 164, p165: 165, p166: 166, p167: 167, p168: 168, p169: 169, p170: 170, p171: 171, p172: 172, p173: 173, p174: 174, p175: 175, p176: 176, p177: 177, p178: 178, p179: 179, p180: 180, p181: 181, p182: 182, p183: 183, p184: 184, p185: 185, p186: 186, p187: 187, p188: 188, p189: 189, p190: 190, p191: 191, p192: 192, p193: 193, p194: 194, p195: 195, p196: 196, p197: 197, p198: 198, p199: 199, p200: 200, p201: 201, p202: 202, p203: 203, p204: 204, p205: 205, p206: 206, p207: 207, p208: 208, p209: 209, p210: 210, p211: 211, p212: 212, p213: 213, p214: 214, p215: 215, p216: 216, p217: 217, p218: 218, p219: 219, p220: 220, p221: 221, p222: 222, p223: 223, p224: 224, p225: 225, p226: 226, p227: 227, p228: 228, p229: 229, p230: 230, p231: 231, p232: 232, p233: 233, p234: 234, p235: 235, p236: 236, p237: 237, p238: 238, p239: 239, p240: 240, p241: 241, p242: 242, p243: 243, p244: 244, p245: 245, p246: 246, p247: 247, p248: 248, p249: 249, p250: 250, p251: 251, p252: 252, p253: 253, p254: 254, p255: 255, p256: 256, p257: 257, p258: 258, p259: 259, p260: 260, p261: 261, p262: 262, p263: 263, p264: 264, p265: 265, p266: 266, p267: 267, p268: 268, p269: 269, p270: 270, p271: 271, p272: 272, p273: 273, p274: 274, p275: 275, p276: 276, p277: 277, p278: 278, p279: 279, p280: 280, p281: 281, p282: 282, p283: 283, p284: 284, p285: 285, p286: 286, p287: 287, p288: 288, p289: 289, p290: 290, p291: 291, p292: 292, p293: 293, p294: 294, p295: 295, p296: 296, p297: 297, p298: 298, p299: 299, p300: 300, p301: 301, p302: 302, p303: 303, p304: 304, p305: 305, p306: 306, p307: 307, p308: 308, p309: 309, p310: 310, p311: 311, p312: 312, p313: 313, p314: 314, p315: 315, p316: 316, p317: 317, p318: 318, p319: 319, p320: 320, p321: 321, p322: 322, p323: 323, p324: 324, p325: 325, p326: 326, p327: 327, p328: 328, p329: 329, p330: 330, p331: 331, p332: 332, p333: 333, p334: 334, p335: 335, p336: 336, p337: 337, p338: 338, p339: 339, p340: 340, p341: 341, p342: 342, p343: 343, p344: 344, p345: 345, p346: 346, p347: 347, p348: 348, p349: 349, p350: 350, p351: 351, p352: 352, p353: 353, p354: 354, p355: 355, p356: 356, p357: 357, p358: 358, p359: 359, p360: 360, p361: 361, p362: 362, p363: 363, p364: 364, p365: 365, p366: 366, p367: 367, p368: 368, p369: 369, p370: 370, p371: 371, p372: 372, p373: 373, p374: 374, p375: 375, p376: 376, p377: 377, p378: 378, p379: 379, p380: 380, p381: 381, p382: 382, p383: 383, p384: 384, p385: 385, p386: 386, p387: 387, p388: 388, p389: 389, p390: 390, p391: 391, p392: 392, p393: 393, p394: 394, p395: 395, p396: 396, p397: 397, p398: 398, p399: 399 }; }
export function lc(a0: string | undefined, a1: number | null, a2: boolean, a3: "x" | "", a4: 0 | 1) { return a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4; }
"##;

/// A function with 100 `if (x === i) return …` statements returns the union of
/// their literals, 49 equality guards narrow a 50-member union to its last
/// member, 50 conditional writes join with the declared initial value, a
/// 400-property object literal widens every member, and a 20-operand `&&` chain
/// unions each operand's falsy part with the last operand.
#[test]
fn long_function_bodies_answer_as_the_checker_answers() {
    let matrix = Matrix::new(LONG_BODIES);
    let mut failures = matrix.same(&[
        (Read::Return("chain"), "\"none\" | \"r0\" | \"r1\" | \"r10\" | \"r11\" | \"r12\" | \"r13\" | \"r14\" | \"r15\" | \"r16\" | \"r17\" | \"r18\" | \"r19\" | \"r2\" | \"r20\" | \"r21\" | \"r22\" | \"r23\" | \"r24\" | \"r25\" | \"r26\" | \"r27\" | \"r28\" | \"r29\" | \"r3\" | \"r30\" | \"r31\" | \"r32\" | \"r33\" | \"r34\" | \"r35\" | \"r36\" | \"r37\" | \"r38\" | \"r39\" | \"r4\" | \"r40\" | \"r41\" | \"r42\" | \"r43\" | \"r44\" | \"r45\" | \"r46\" | \"r47\" | \"r48\" | \"r49\" | \"r5\" | \"r50\" | \"r51\" | \"r52\" | \"r53\" | \"r54\" | \"r55\" | \"r56\" | \"r57\" | \"r58\" | \"r59\" | \"r6\" | \"r60\" | \"r61\" | \"r62\" | \"r63\" | \"r64\" | \"r65\" | \"r66\" | \"r67\" | \"r68\" | \"r69\" | \"r7\" | \"r70\" | \"r71\" | \"r72\" | \"r73\" | \"r74\" | \"r75\" | \"r76\" | \"r77\" | \"r78\" | \"r79\" | \"r8\" | \"r80\" | \"r81\" | \"r82\" | \"r83\" | \"r84\" | \"r85\" | \"r86\" | \"r87\" | \"r88\" | \"r89\" | \"r9\" | \"r90\" | \"r91\" | \"r92\" | \"r93\" | \"r94\" | \"r95\" | \"r96\" | \"r97\" | \"r98\" | \"r99\""),
        (Read::Return("last"), "\"k49\""),
        (Read::Return("acc"), "string | number"),
        (Read::Return("obj"), "{ p0: number; p1: number; p2: number; p3: number; p4: number; p5: number; p6: number; p7: number; p8: number; p9: number; p10: number; p11: number; p12: number; p13: number; p14: number; p15: number; p16: number; p17: number; p18: number; p19: number; p20: number; p21: number; p22: number; p23: number; p24: number; p25: number; p26: number; p27: number; p28: number; p29: number; p30: number; p31: number; p32: number; p33: number; p34: number; p35: number; p36: number; p37: number; p38: number; p39: number; p40: number; p41: number; p42: number; p43: number; p44: number; p45: number; p46: number; p47: number; p48: number; p49: number; p50: number; p51: number; p52: number; p53: number; p54: number; p55: number; p56: number; p57: number; p58: number; p59: number; p60: number; p61: number; p62: number; p63: number; p64: number; p65: number; p66: number; p67: number; p68: number; p69: number; p70: number; p71: number; p72: number; p73: number; p74: number; p75: number; p76: number; p77: number; p78: number; p79: number; p80: number; p81: number; p82: number; p83: number; p84: number; p85: number; p86: number; p87: number; p88: number; p89: number; p90: number; p91: number; p92: number; p93: number; p94: number; p95: number; p96: number; p97: number; p98: number; p99: number; p100: number; p101: number; p102: number; p103: number; p104: number; p105: number; p106: number; p107: number; p108: number; p109: number; p110: number; p111: number; p112: number; p113: number; p114: number; p115: number; p116: number; p117: number; p118: number; p119: number; p120: number; p121: number; p122: number; p123: number; p124: number; p125: number; p126: number; p127: number; p128: number; p129: number; p130: number; p131: number; p132: number; p133: number; p134: number; p135: number; p136: number; p137: number; p138: number; p139: number; p140: number; p141: number; p142: number; p143: number; p144: number; p145: number; p146: number; p147: number; p148: number; p149: number; p150: number; p151: number; p152: number; p153: number; p154: number; p155: number; p156: number; p157: number; p158: number; p159: number; p160: number; p161: number; p162: number; p163: number; p164: number; p165: number; p166: number; p167: number; p168: number; p169: number; p170: number; p171: number; p172: number; p173: number; p174: number; p175: number; p176: number; p177: number; p178: number; p179: number; p180: number; p181: number; p182: number; p183: number; p184: number; p185: number; p186: number; p187: number; p188: number; p189: number; p190: number; p191: number; p192: number; p193: number; p194: number; p195: number; p196: number; p197: number; p198: number; p199: number; p200: number; p201: number; p202: number; p203: number; p204: number; p205: number; p206: number; p207: number; p208: number; p209: number; p210: number; p211: number; p212: number; p213: number; p214: number; p215: number; p216: number; p217: number; p218: number; p219: number; p220: number; p221: number; p222: number; p223: number; p224: number; p225: number; p226: number; p227: number; p228: number; p229: number; p230: number; p231: number; p232: number; p233: number; p234: number; p235: number; p236: number; p237: number; p238: number; p239: number; p240: number; p241: number; p242: number; p243: number; p244: number; p245: number; p246: number; p247: number; p248: number; p249: number; p250: number; p251: number; p252: number; p253: number; p254: number; p255: number; p256: number; p257: number; p258: number; p259: number; p260: number; p261: number; p262: number; p263: number; p264: number; p265: number; p266: number; p267: number; p268: number; p269: number; p270: number; p271: number; p272: number; p273: number; p274: number; p275: number; p276: number; p277: number; p278: number; p279: number; p280: number; p281: number; p282: number; p283: number; p284: number; p285: number; p286: number; p287: number; p288: number; p289: number; p290: number; p291: number; p292: number; p293: number; p294: number; p295: number; p296: number; p297: number; p298: number; p299: number; p300: number; p301: number; p302: number; p303: number; p304: number; p305: number; p306: number; p307: number; p308: number; p309: number; p310: number; p311: number; p312: number; p313: number; p314: number; p315: number; p316: number; p317: number; p318: number; p319: number; p320: number; p321: number; p322: number; p323: number; p324: number; p325: number; p326: number; p327: number; p328: number; p329: number; p330: number; p331: number; p332: number; p333: number; p334: number; p335: number; p336: number; p337: number; p338: number; p339: number; p340: number; p341: number; p342: number; p343: number; p344: number; p345: number; p346: number; p347: number; p348: number; p349: number; p350: number; p351: number; p352: number; p353: number; p354: number; p355: number; p356: number; p357: number; p358: number; p359: number; p360: number; p361: number; p362: number; p363: number; p364: number; p365: number; p366: number; p367: number; p368: number; p369: number; p370: number; p371: number; p372: number; p373: number; p374: number; p375: number; p376: number; p377: number; p378: number; p379: number; p380: number; p381: number; p382: number; p383: number; p384: number; p385: number; p386: number; p387: number; p388: number; p389: number; p390: number; p391: number; p392: number; p393: number; p394: number; p395: number; p396: number; p397: number; p398: number; p399: number; }"),
        (Read::Type("ReturnType<typeof obj>['p399']"), "number"),
    ]);
    failures.extend(matrix.nullness(&[(
        Read::Return("lc"),
        "\"\" | 0 | 1 | false | null | undefined",
        "0 | 1",
    )]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `type D` is `Box<…>` applied 80 times around `1`.
const BOX_80: &str = r##"interface Box<T> { v: T }
type D = Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<1>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>;
"##;

/// `[D] extends [<Box<…> applied 80 times around number>] ? 1 : 2` is `1`. The
/// lane overflows its stack and aborts the process (the relation to the
/// identical 80-deep type overflows too; eighty `['v']` reads through `D`
/// overflowed in one run and answered in others, so they sit at the edge; 72
/// levels answer in 0.3 s). Run it alone.
///
/// What the lane gives:
/// - `[D] extends
///   [Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<number>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>]
///   ? 1 : 2`: the checker answers `1`; the lane the process aborts: `thread
///   '<unknown>' has overflowed its stack`.
#[test]
#[ignore = "run alone: relating an 80-deep generic application overflows the stack"]
fn an_80_deep_nested_generic_application_relates_on_the_default_stack() {
    let matrix = Matrix::new(BOX_80);
    let failures = matrix.types(&[
        ("[D] extends [Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<Box<number>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>] ? 1 : 2", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An 800-member literal union.
const WIDE_UNION_800: &str = r##"type W = "m0" | "m1" | "m2" | "m3" | "m4" | "m5" | "m6" | "m7" | "m8" | "m9" | "m10" | "m11" | "m12" | "m13" | "m14" | "m15" | "m16" | "m17" | "m18" | "m19" | "m20" | "m21" | "m22" | "m23" | "m24" | "m25" | "m26" | "m27" | "m28" | "m29" | "m30" | "m31" | "m32" | "m33" | "m34" | "m35" | "m36" | "m37" | "m38" | "m39" | "m40" | "m41" | "m42" | "m43" | "m44" | "m45" | "m46" | "m47" | "m48" | "m49" | "m50" | "m51" | "m52" | "m53" | "m54" | "m55" | "m56" | "m57" | "m58" | "m59" | "m60" | "m61" | "m62" | "m63" | "m64" | "m65" | "m66" | "m67" | "m68" | "m69" | "m70" | "m71" | "m72" | "m73" | "m74" | "m75" | "m76" | "m77" | "m78" | "m79" | "m80" | "m81" | "m82" | "m83" | "m84" | "m85" | "m86" | "m87" | "m88" | "m89" | "m90" | "m91" | "m92" | "m93" | "m94" | "m95" | "m96" | "m97" | "m98" | "m99" | "m100" | "m101" | "m102" | "m103" | "m104" | "m105" | "m106" | "m107" | "m108" | "m109" | "m110" | "m111" | "m112" | "m113" | "m114" | "m115" | "m116" | "m117" | "m118" | "m119" | "m120" | "m121" | "m122" | "m123" | "m124" | "m125" | "m126" | "m127" | "m128" | "m129" | "m130" | "m131" | "m132" | "m133" | "m134" | "m135" | "m136" | "m137" | "m138" | "m139" | "m140" | "m141" | "m142" | "m143" | "m144" | "m145" | "m146" | "m147" | "m148" | "m149" | "m150" | "m151" | "m152" | "m153" | "m154" | "m155" | "m156" | "m157" | "m158" | "m159" | "m160" | "m161" | "m162" | "m163" | "m164" | "m165" | "m166" | "m167" | "m168" | "m169" | "m170" | "m171" | "m172" | "m173" | "m174" | "m175" | "m176" | "m177" | "m178" | "m179" | "m180" | "m181" | "m182" | "m183" | "m184" | "m185" | "m186" | "m187" | "m188" | "m189" | "m190" | "m191" | "m192" | "m193" | "m194" | "m195" | "m196" | "m197" | "m198" | "m199" | "m200" | "m201" | "m202" | "m203" | "m204" | "m205" | "m206" | "m207" | "m208" | "m209" | "m210" | "m211" | "m212" | "m213" | "m214" | "m215" | "m216" | "m217" | "m218" | "m219" | "m220" | "m221" | "m222" | "m223" | "m224" | "m225" | "m226" | "m227" | "m228" | "m229" | "m230" | "m231" | "m232" | "m233" | "m234" | "m235" | "m236" | "m237" | "m238" | "m239" | "m240" | "m241" | "m242" | "m243" | "m244" | "m245" | "m246" | "m247" | "m248" | "m249" | "m250" | "m251" | "m252" | "m253" | "m254" | "m255" | "m256" | "m257" | "m258" | "m259" | "m260" | "m261" | "m262" | "m263" | "m264" | "m265" | "m266" | "m267" | "m268" | "m269" | "m270" | "m271" | "m272" | "m273" | "m274" | "m275" | "m276" | "m277" | "m278" | "m279" | "m280" | "m281" | "m282" | "m283" | "m284" | "m285" | "m286" | "m287" | "m288" | "m289" | "m290" | "m291" | "m292" | "m293" | "m294" | "m295" | "m296" | "m297" | "m298" | "m299" | "m300" | "m301" | "m302" | "m303" | "m304" | "m305" | "m306" | "m307" | "m308" | "m309" | "m310" | "m311" | "m312" | "m313" | "m314" | "m315" | "m316" | "m317" | "m318" | "m319" | "m320" | "m321" | "m322" | "m323" | "m324" | "m325" | "m326" | "m327" | "m328" | "m329" | "m330" | "m331" | "m332" | "m333" | "m334" | "m335" | "m336" | "m337" | "m338" | "m339" | "m340" | "m341" | "m342" | "m343" | "m344" | "m345" | "m346" | "m347" | "m348" | "m349" | "m350" | "m351" | "m352" | "m353" | "m354" | "m355" | "m356" | "m357" | "m358" | "m359" | "m360" | "m361" | "m362" | "m363" | "m364" | "m365" | "m366" | "m367" | "m368" | "m369" | "m370" | "m371" | "m372" | "m373" | "m374" | "m375" | "m376" | "m377" | "m378" | "m379" | "m380" | "m381" | "m382" | "m383" | "m384" | "m385" | "m386" | "m387" | "m388" | "m389" | "m390" | "m391" | "m392" | "m393" | "m394" | "m395" | "m396" | "m397" | "m398" | "m399" | "m400" | "m401" | "m402" | "m403" | "m404" | "m405" | "m406" | "m407" | "m408" | "m409" | "m410" | "m411" | "m412" | "m413" | "m414" | "m415" | "m416" | "m417" | "m418" | "m419" | "m420" | "m421" | "m422" | "m423" | "m424" | "m425" | "m426" | "m427" | "m428" | "m429" | "m430" | "m431" | "m432" | "m433" | "m434" | "m435" | "m436" | "m437" | "m438" | "m439" | "m440" | "m441" | "m442" | "m443" | "m444" | "m445" | "m446" | "m447" | "m448" | "m449" | "m450" | "m451" | "m452" | "m453" | "m454" | "m455" | "m456" | "m457" | "m458" | "m459" | "m460" | "m461" | "m462" | "m463" | "m464" | "m465" | "m466" | "m467" | "m468" | "m469" | "m470" | "m471" | "m472" | "m473" | "m474" | "m475" | "m476" | "m477" | "m478" | "m479" | "m480" | "m481" | "m482" | "m483" | "m484" | "m485" | "m486" | "m487" | "m488" | "m489" | "m490" | "m491" | "m492" | "m493" | "m494" | "m495" | "m496" | "m497" | "m498" | "m499" | "m500" | "m501" | "m502" | "m503" | "m504" | "m505" | "m506" | "m507" | "m508" | "m509" | "m510" | "m511" | "m512" | "m513" | "m514" | "m515" | "m516" | "m517" | "m518" | "m519" | "m520" | "m521" | "m522" | "m523" | "m524" | "m525" | "m526" | "m527" | "m528" | "m529" | "m530" | "m531" | "m532" | "m533" | "m534" | "m535" | "m536" | "m537" | "m538" | "m539" | "m540" | "m541" | "m542" | "m543" | "m544" | "m545" | "m546" | "m547" | "m548" | "m549" | "m550" | "m551" | "m552" | "m553" | "m554" | "m555" | "m556" | "m557" | "m558" | "m559" | "m560" | "m561" | "m562" | "m563" | "m564" | "m565" | "m566" | "m567" | "m568" | "m569" | "m570" | "m571" | "m572" | "m573" | "m574" | "m575" | "m576" | "m577" | "m578" | "m579" | "m580" | "m581" | "m582" | "m583" | "m584" | "m585" | "m586" | "m587" | "m588" | "m589" | "m590" | "m591" | "m592" | "m593" | "m594" | "m595" | "m596" | "m597" | "m598" | "m599" | "m600" | "m601" | "m602" | "m603" | "m604" | "m605" | "m606" | "m607" | "m608" | "m609" | "m610" | "m611" | "m612" | "m613" | "m614" | "m615" | "m616" | "m617" | "m618" | "m619" | "m620" | "m621" | "m622" | "m623" | "m624" | "m625" | "m626" | "m627" | "m628" | "m629" | "m630" | "m631" | "m632" | "m633" | "m634" | "m635" | "m636" | "m637" | "m638" | "m639" | "m640" | "m641" | "m642" | "m643" | "m644" | "m645" | "m646" | "m647" | "m648" | "m649" | "m650" | "m651" | "m652" | "m653" | "m654" | "m655" | "m656" | "m657" | "m658" | "m659" | "m660" | "m661" | "m662" | "m663" | "m664" | "m665" | "m666" | "m667" | "m668" | "m669" | "m670" | "m671" | "m672" | "m673" | "m674" | "m675" | "m676" | "m677" | "m678" | "m679" | "m680" | "m681" | "m682" | "m683" | "m684" | "m685" | "m686" | "m687" | "m688" | "m689" | "m690" | "m691" | "m692" | "m693" | "m694" | "m695" | "m696" | "m697" | "m698" | "m699" | "m700" | "m701" | "m702" | "m703" | "m704" | "m705" | "m706" | "m707" | "m708" | "m709" | "m710" | "m711" | "m712" | "m713" | "m714" | "m715" | "m716" | "m717" | "m718" | "m719" | "m720" | "m721" | "m722" | "m723" | "m724" | "m725" | "m726" | "m727" | "m728" | "m729" | "m730" | "m731" | "m732" | "m733" | "m734" | "m735" | "m736" | "m737" | "m738" | "m739" | "m740" | "m741" | "m742" | "m743" | "m744" | "m745" | "m746" | "m747" | "m748" | "m749" | "m750" | "m751" | "m752" | "m753" | "m754" | "m755" | "m756" | "m757" | "m758" | "m759" | "m760" | "m761" | "m762" | "m763" | "m764" | "m765" | "m766" | "m767" | "m768" | "m769" | "m770" | "m771" | "m772" | "m773" | "m774" | "m775" | "m776" | "m777" | "m778" | "m779" | "m780" | "m781" | "m782" | "m783" | "m784" | "m785" | "m786" | "m787" | "m788" | "m789" | "m790" | "m791" | "m792" | "m793" | "m794" | "m795" | "m796" | "m797" | "m798" | "m799";
"##;

/// `[Exclude<W, "m0">] extends [W] ? 1 : 2` over an 800-member union is `1`.
/// The lane's time grows about cubically with the union: 100 members 0.9 s, 200
/// 5.0 s, 400 45.2 s for the four settings; at 800 every setting passes the row
/// deadline.
///
/// What the lane gives:
/// - `[Exclude<W, "m0">] extends [W] ? 1 : 2`: the checker answers `1`; the
///   lane took longer than 60s.
#[test]
#[ignore = "Exclude over a wide literal union relates in time the checker takes"]
fn excluding_one_member_of_an_800_member_union_answers() {
    let matrix = Matrix::new(WIDE_UNION_800);
    let failures = matrix.types(&[("[Exclude<W, \"m0\">] extends [W] ? 1 : 2", "1")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// 799 equality guards over an 800-member literal union.
const NARROW_CHAIN_800: &str = r##"type K = "k0" | "k1" | "k2" | "k3" | "k4" | "k5" | "k6" | "k7" | "k8" | "k9" | "k10" | "k11" | "k12" | "k13" | "k14" | "k15" | "k16" | "k17" | "k18" | "k19" | "k20" | "k21" | "k22" | "k23" | "k24" | "k25" | "k26" | "k27" | "k28" | "k29" | "k30" | "k31" | "k32" | "k33" | "k34" | "k35" | "k36" | "k37" | "k38" | "k39" | "k40" | "k41" | "k42" | "k43" | "k44" | "k45" | "k46" | "k47" | "k48" | "k49" | "k50" | "k51" | "k52" | "k53" | "k54" | "k55" | "k56" | "k57" | "k58" | "k59" | "k60" | "k61" | "k62" | "k63" | "k64" | "k65" | "k66" | "k67" | "k68" | "k69" | "k70" | "k71" | "k72" | "k73" | "k74" | "k75" | "k76" | "k77" | "k78" | "k79" | "k80" | "k81" | "k82" | "k83" | "k84" | "k85" | "k86" | "k87" | "k88" | "k89" | "k90" | "k91" | "k92" | "k93" | "k94" | "k95" | "k96" | "k97" | "k98" | "k99" | "k100" | "k101" | "k102" | "k103" | "k104" | "k105" | "k106" | "k107" | "k108" | "k109" | "k110" | "k111" | "k112" | "k113" | "k114" | "k115" | "k116" | "k117" | "k118" | "k119" | "k120" | "k121" | "k122" | "k123" | "k124" | "k125" | "k126" | "k127" | "k128" | "k129" | "k130" | "k131" | "k132" | "k133" | "k134" | "k135" | "k136" | "k137" | "k138" | "k139" | "k140" | "k141" | "k142" | "k143" | "k144" | "k145" | "k146" | "k147" | "k148" | "k149" | "k150" | "k151" | "k152" | "k153" | "k154" | "k155" | "k156" | "k157" | "k158" | "k159" | "k160" | "k161" | "k162" | "k163" | "k164" | "k165" | "k166" | "k167" | "k168" | "k169" | "k170" | "k171" | "k172" | "k173" | "k174" | "k175" | "k176" | "k177" | "k178" | "k179" | "k180" | "k181" | "k182" | "k183" | "k184" | "k185" | "k186" | "k187" | "k188" | "k189" | "k190" | "k191" | "k192" | "k193" | "k194" | "k195" | "k196" | "k197" | "k198" | "k199" | "k200" | "k201" | "k202" | "k203" | "k204" | "k205" | "k206" | "k207" | "k208" | "k209" | "k210" | "k211" | "k212" | "k213" | "k214" | "k215" | "k216" | "k217" | "k218" | "k219" | "k220" | "k221" | "k222" | "k223" | "k224" | "k225" | "k226" | "k227" | "k228" | "k229" | "k230" | "k231" | "k232" | "k233" | "k234" | "k235" | "k236" | "k237" | "k238" | "k239" | "k240" | "k241" | "k242" | "k243" | "k244" | "k245" | "k246" | "k247" | "k248" | "k249" | "k250" | "k251" | "k252" | "k253" | "k254" | "k255" | "k256" | "k257" | "k258" | "k259" | "k260" | "k261" | "k262" | "k263" | "k264" | "k265" | "k266" | "k267" | "k268" | "k269" | "k270" | "k271" | "k272" | "k273" | "k274" | "k275" | "k276" | "k277" | "k278" | "k279" | "k280" | "k281" | "k282" | "k283" | "k284" | "k285" | "k286" | "k287" | "k288" | "k289" | "k290" | "k291" | "k292" | "k293" | "k294" | "k295" | "k296" | "k297" | "k298" | "k299" | "k300" | "k301" | "k302" | "k303" | "k304" | "k305" | "k306" | "k307" | "k308" | "k309" | "k310" | "k311" | "k312" | "k313" | "k314" | "k315" | "k316" | "k317" | "k318" | "k319" | "k320" | "k321" | "k322" | "k323" | "k324" | "k325" | "k326" | "k327" | "k328" | "k329" | "k330" | "k331" | "k332" | "k333" | "k334" | "k335" | "k336" | "k337" | "k338" | "k339" | "k340" | "k341" | "k342" | "k343" | "k344" | "k345" | "k346" | "k347" | "k348" | "k349" | "k350" | "k351" | "k352" | "k353" | "k354" | "k355" | "k356" | "k357" | "k358" | "k359" | "k360" | "k361" | "k362" | "k363" | "k364" | "k365" | "k366" | "k367" | "k368" | "k369" | "k370" | "k371" | "k372" | "k373" | "k374" | "k375" | "k376" | "k377" | "k378" | "k379" | "k380" | "k381" | "k382" | "k383" | "k384" | "k385" | "k386" | "k387" | "k388" | "k389" | "k390" | "k391" | "k392" | "k393" | "k394" | "k395" | "k396" | "k397" | "k398" | "k399" | "k400" | "k401" | "k402" | "k403" | "k404" | "k405" | "k406" | "k407" | "k408" | "k409" | "k410" | "k411" | "k412" | "k413" | "k414" | "k415" | "k416" | "k417" | "k418" | "k419" | "k420" | "k421" | "k422" | "k423" | "k424" | "k425" | "k426" | "k427" | "k428" | "k429" | "k430" | "k431" | "k432" | "k433" | "k434" | "k435" | "k436" | "k437" | "k438" | "k439" | "k440" | "k441" | "k442" | "k443" | "k444" | "k445" | "k446" | "k447" | "k448" | "k449" | "k450" | "k451" | "k452" | "k453" | "k454" | "k455" | "k456" | "k457" | "k458" | "k459" | "k460" | "k461" | "k462" | "k463" | "k464" | "k465" | "k466" | "k467" | "k468" | "k469" | "k470" | "k471" | "k472" | "k473" | "k474" | "k475" | "k476" | "k477" | "k478" | "k479" | "k480" | "k481" | "k482" | "k483" | "k484" | "k485" | "k486" | "k487" | "k488" | "k489" | "k490" | "k491" | "k492" | "k493" | "k494" | "k495" | "k496" | "k497" | "k498" | "k499" | "k500" | "k501" | "k502" | "k503" | "k504" | "k505" | "k506" | "k507" | "k508" | "k509" | "k510" | "k511" | "k512" | "k513" | "k514" | "k515" | "k516" | "k517" | "k518" | "k519" | "k520" | "k521" | "k522" | "k523" | "k524" | "k525" | "k526" | "k527" | "k528" | "k529" | "k530" | "k531" | "k532" | "k533" | "k534" | "k535" | "k536" | "k537" | "k538" | "k539" | "k540" | "k541" | "k542" | "k543" | "k544" | "k545" | "k546" | "k547" | "k548" | "k549" | "k550" | "k551" | "k552" | "k553" | "k554" | "k555" | "k556" | "k557" | "k558" | "k559" | "k560" | "k561" | "k562" | "k563" | "k564" | "k565" | "k566" | "k567" | "k568" | "k569" | "k570" | "k571" | "k572" | "k573" | "k574" | "k575" | "k576" | "k577" | "k578" | "k579" | "k580" | "k581" | "k582" | "k583" | "k584" | "k585" | "k586" | "k587" | "k588" | "k589" | "k590" | "k591" | "k592" | "k593" | "k594" | "k595" | "k596" | "k597" | "k598" | "k599" | "k600" | "k601" | "k602" | "k603" | "k604" | "k605" | "k606" | "k607" | "k608" | "k609" | "k610" | "k611" | "k612" | "k613" | "k614" | "k615" | "k616" | "k617" | "k618" | "k619" | "k620" | "k621" | "k622" | "k623" | "k624" | "k625" | "k626" | "k627" | "k628" | "k629" | "k630" | "k631" | "k632" | "k633" | "k634" | "k635" | "k636" | "k637" | "k638" | "k639" | "k640" | "k641" | "k642" | "k643" | "k644" | "k645" | "k646" | "k647" | "k648" | "k649" | "k650" | "k651" | "k652" | "k653" | "k654" | "k655" | "k656" | "k657" | "k658" | "k659" | "k660" | "k661" | "k662" | "k663" | "k664" | "k665" | "k666" | "k667" | "k668" | "k669" | "k670" | "k671" | "k672" | "k673" | "k674" | "k675" | "k676" | "k677" | "k678" | "k679" | "k680" | "k681" | "k682" | "k683" | "k684" | "k685" | "k686" | "k687" | "k688" | "k689" | "k690" | "k691" | "k692" | "k693" | "k694" | "k695" | "k696" | "k697" | "k698" | "k699" | "k700" | "k701" | "k702" | "k703" | "k704" | "k705" | "k706" | "k707" | "k708" | "k709" | "k710" | "k711" | "k712" | "k713" | "k714" | "k715" | "k716" | "k717" | "k718" | "k719" | "k720" | "k721" | "k722" | "k723" | "k724" | "k725" | "k726" | "k727" | "k728" | "k729" | "k730" | "k731" | "k732" | "k733" | "k734" | "k735" | "k736" | "k737" | "k738" | "k739" | "k740" | "k741" | "k742" | "k743" | "k744" | "k745" | "k746" | "k747" | "k748" | "k749" | "k750" | "k751" | "k752" | "k753" | "k754" | "k755" | "k756" | "k757" | "k758" | "k759" | "k760" | "k761" | "k762" | "k763" | "k764" | "k765" | "k766" | "k767" | "k768" | "k769" | "k770" | "k771" | "k772" | "k773" | "k774" | "k775" | "k776" | "k777" | "k778" | "k779" | "k780" | "k781" | "k782" | "k783" | "k784" | "k785" | "k786" | "k787" | "k788" | "k789" | "k790" | "k791" | "k792" | "k793" | "k794" | "k795" | "k796" | "k797" | "k798" | "k799";
export function last(x: K) {
  if (x === "k0") throw 0;
  if (x === "k1") throw 0;
  if (x === "k2") throw 0;
  if (x === "k3") throw 0;
  if (x === "k4") throw 0;
  if (x === "k5") throw 0;
  if (x === "k6") throw 0;
  if (x === "k7") throw 0;
  if (x === "k8") throw 0;
  if (x === "k9") throw 0;
  if (x === "k10") throw 0;
  if (x === "k11") throw 0;
  if (x === "k12") throw 0;
  if (x === "k13") throw 0;
  if (x === "k14") throw 0;
  if (x === "k15") throw 0;
  if (x === "k16") throw 0;
  if (x === "k17") throw 0;
  if (x === "k18") throw 0;
  if (x === "k19") throw 0;
  if (x === "k20") throw 0;
  if (x === "k21") throw 0;
  if (x === "k22") throw 0;
  if (x === "k23") throw 0;
  if (x === "k24") throw 0;
  if (x === "k25") throw 0;
  if (x === "k26") throw 0;
  if (x === "k27") throw 0;
  if (x === "k28") throw 0;
  if (x === "k29") throw 0;
  if (x === "k30") throw 0;
  if (x === "k31") throw 0;
  if (x === "k32") throw 0;
  if (x === "k33") throw 0;
  if (x === "k34") throw 0;
  if (x === "k35") throw 0;
  if (x === "k36") throw 0;
  if (x === "k37") throw 0;
  if (x === "k38") throw 0;
  if (x === "k39") throw 0;
  if (x === "k40") throw 0;
  if (x === "k41") throw 0;
  if (x === "k42") throw 0;
  if (x === "k43") throw 0;
  if (x === "k44") throw 0;
  if (x === "k45") throw 0;
  if (x === "k46") throw 0;
  if (x === "k47") throw 0;
  if (x === "k48") throw 0;
  if (x === "k49") throw 0;
  if (x === "k50") throw 0;
  if (x === "k51") throw 0;
  if (x === "k52") throw 0;
  if (x === "k53") throw 0;
  if (x === "k54") throw 0;
  if (x === "k55") throw 0;
  if (x === "k56") throw 0;
  if (x === "k57") throw 0;
  if (x === "k58") throw 0;
  if (x === "k59") throw 0;
  if (x === "k60") throw 0;
  if (x === "k61") throw 0;
  if (x === "k62") throw 0;
  if (x === "k63") throw 0;
  if (x === "k64") throw 0;
  if (x === "k65") throw 0;
  if (x === "k66") throw 0;
  if (x === "k67") throw 0;
  if (x === "k68") throw 0;
  if (x === "k69") throw 0;
  if (x === "k70") throw 0;
  if (x === "k71") throw 0;
  if (x === "k72") throw 0;
  if (x === "k73") throw 0;
  if (x === "k74") throw 0;
  if (x === "k75") throw 0;
  if (x === "k76") throw 0;
  if (x === "k77") throw 0;
  if (x === "k78") throw 0;
  if (x === "k79") throw 0;
  if (x === "k80") throw 0;
  if (x === "k81") throw 0;
  if (x === "k82") throw 0;
  if (x === "k83") throw 0;
  if (x === "k84") throw 0;
  if (x === "k85") throw 0;
  if (x === "k86") throw 0;
  if (x === "k87") throw 0;
  if (x === "k88") throw 0;
  if (x === "k89") throw 0;
  if (x === "k90") throw 0;
  if (x === "k91") throw 0;
  if (x === "k92") throw 0;
  if (x === "k93") throw 0;
  if (x === "k94") throw 0;
  if (x === "k95") throw 0;
  if (x === "k96") throw 0;
  if (x === "k97") throw 0;
  if (x === "k98") throw 0;
  if (x === "k99") throw 0;
  if (x === "k100") throw 0;
  if (x === "k101") throw 0;
  if (x === "k102") throw 0;
  if (x === "k103") throw 0;
  if (x === "k104") throw 0;
  if (x === "k105") throw 0;
  if (x === "k106") throw 0;
  if (x === "k107") throw 0;
  if (x === "k108") throw 0;
  if (x === "k109") throw 0;
  if (x === "k110") throw 0;
  if (x === "k111") throw 0;
  if (x === "k112") throw 0;
  if (x === "k113") throw 0;
  if (x === "k114") throw 0;
  if (x === "k115") throw 0;
  if (x === "k116") throw 0;
  if (x === "k117") throw 0;
  if (x === "k118") throw 0;
  if (x === "k119") throw 0;
  if (x === "k120") throw 0;
  if (x === "k121") throw 0;
  if (x === "k122") throw 0;
  if (x === "k123") throw 0;
  if (x === "k124") throw 0;
  if (x === "k125") throw 0;
  if (x === "k126") throw 0;
  if (x === "k127") throw 0;
  if (x === "k128") throw 0;
  if (x === "k129") throw 0;
  if (x === "k130") throw 0;
  if (x === "k131") throw 0;
  if (x === "k132") throw 0;
  if (x === "k133") throw 0;
  if (x === "k134") throw 0;
  if (x === "k135") throw 0;
  if (x === "k136") throw 0;
  if (x === "k137") throw 0;
  if (x === "k138") throw 0;
  if (x === "k139") throw 0;
  if (x === "k140") throw 0;
  if (x === "k141") throw 0;
  if (x === "k142") throw 0;
  if (x === "k143") throw 0;
  if (x === "k144") throw 0;
  if (x === "k145") throw 0;
  if (x === "k146") throw 0;
  if (x === "k147") throw 0;
  if (x === "k148") throw 0;
  if (x === "k149") throw 0;
  if (x === "k150") throw 0;
  if (x === "k151") throw 0;
  if (x === "k152") throw 0;
  if (x === "k153") throw 0;
  if (x === "k154") throw 0;
  if (x === "k155") throw 0;
  if (x === "k156") throw 0;
  if (x === "k157") throw 0;
  if (x === "k158") throw 0;
  if (x === "k159") throw 0;
  if (x === "k160") throw 0;
  if (x === "k161") throw 0;
  if (x === "k162") throw 0;
  if (x === "k163") throw 0;
  if (x === "k164") throw 0;
  if (x === "k165") throw 0;
  if (x === "k166") throw 0;
  if (x === "k167") throw 0;
  if (x === "k168") throw 0;
  if (x === "k169") throw 0;
  if (x === "k170") throw 0;
  if (x === "k171") throw 0;
  if (x === "k172") throw 0;
  if (x === "k173") throw 0;
  if (x === "k174") throw 0;
  if (x === "k175") throw 0;
  if (x === "k176") throw 0;
  if (x === "k177") throw 0;
  if (x === "k178") throw 0;
  if (x === "k179") throw 0;
  if (x === "k180") throw 0;
  if (x === "k181") throw 0;
  if (x === "k182") throw 0;
  if (x === "k183") throw 0;
  if (x === "k184") throw 0;
  if (x === "k185") throw 0;
  if (x === "k186") throw 0;
  if (x === "k187") throw 0;
  if (x === "k188") throw 0;
  if (x === "k189") throw 0;
  if (x === "k190") throw 0;
  if (x === "k191") throw 0;
  if (x === "k192") throw 0;
  if (x === "k193") throw 0;
  if (x === "k194") throw 0;
  if (x === "k195") throw 0;
  if (x === "k196") throw 0;
  if (x === "k197") throw 0;
  if (x === "k198") throw 0;
  if (x === "k199") throw 0;
  if (x === "k200") throw 0;
  if (x === "k201") throw 0;
  if (x === "k202") throw 0;
  if (x === "k203") throw 0;
  if (x === "k204") throw 0;
  if (x === "k205") throw 0;
  if (x === "k206") throw 0;
  if (x === "k207") throw 0;
  if (x === "k208") throw 0;
  if (x === "k209") throw 0;
  if (x === "k210") throw 0;
  if (x === "k211") throw 0;
  if (x === "k212") throw 0;
  if (x === "k213") throw 0;
  if (x === "k214") throw 0;
  if (x === "k215") throw 0;
  if (x === "k216") throw 0;
  if (x === "k217") throw 0;
  if (x === "k218") throw 0;
  if (x === "k219") throw 0;
  if (x === "k220") throw 0;
  if (x === "k221") throw 0;
  if (x === "k222") throw 0;
  if (x === "k223") throw 0;
  if (x === "k224") throw 0;
  if (x === "k225") throw 0;
  if (x === "k226") throw 0;
  if (x === "k227") throw 0;
  if (x === "k228") throw 0;
  if (x === "k229") throw 0;
  if (x === "k230") throw 0;
  if (x === "k231") throw 0;
  if (x === "k232") throw 0;
  if (x === "k233") throw 0;
  if (x === "k234") throw 0;
  if (x === "k235") throw 0;
  if (x === "k236") throw 0;
  if (x === "k237") throw 0;
  if (x === "k238") throw 0;
  if (x === "k239") throw 0;
  if (x === "k240") throw 0;
  if (x === "k241") throw 0;
  if (x === "k242") throw 0;
  if (x === "k243") throw 0;
  if (x === "k244") throw 0;
  if (x === "k245") throw 0;
  if (x === "k246") throw 0;
  if (x === "k247") throw 0;
  if (x === "k248") throw 0;
  if (x === "k249") throw 0;
  if (x === "k250") throw 0;
  if (x === "k251") throw 0;
  if (x === "k252") throw 0;
  if (x === "k253") throw 0;
  if (x === "k254") throw 0;
  if (x === "k255") throw 0;
  if (x === "k256") throw 0;
  if (x === "k257") throw 0;
  if (x === "k258") throw 0;
  if (x === "k259") throw 0;
  if (x === "k260") throw 0;
  if (x === "k261") throw 0;
  if (x === "k262") throw 0;
  if (x === "k263") throw 0;
  if (x === "k264") throw 0;
  if (x === "k265") throw 0;
  if (x === "k266") throw 0;
  if (x === "k267") throw 0;
  if (x === "k268") throw 0;
  if (x === "k269") throw 0;
  if (x === "k270") throw 0;
  if (x === "k271") throw 0;
  if (x === "k272") throw 0;
  if (x === "k273") throw 0;
  if (x === "k274") throw 0;
  if (x === "k275") throw 0;
  if (x === "k276") throw 0;
  if (x === "k277") throw 0;
  if (x === "k278") throw 0;
  if (x === "k279") throw 0;
  if (x === "k280") throw 0;
  if (x === "k281") throw 0;
  if (x === "k282") throw 0;
  if (x === "k283") throw 0;
  if (x === "k284") throw 0;
  if (x === "k285") throw 0;
  if (x === "k286") throw 0;
  if (x === "k287") throw 0;
  if (x === "k288") throw 0;
  if (x === "k289") throw 0;
  if (x === "k290") throw 0;
  if (x === "k291") throw 0;
  if (x === "k292") throw 0;
  if (x === "k293") throw 0;
  if (x === "k294") throw 0;
  if (x === "k295") throw 0;
  if (x === "k296") throw 0;
  if (x === "k297") throw 0;
  if (x === "k298") throw 0;
  if (x === "k299") throw 0;
  if (x === "k300") throw 0;
  if (x === "k301") throw 0;
  if (x === "k302") throw 0;
  if (x === "k303") throw 0;
  if (x === "k304") throw 0;
  if (x === "k305") throw 0;
  if (x === "k306") throw 0;
  if (x === "k307") throw 0;
  if (x === "k308") throw 0;
  if (x === "k309") throw 0;
  if (x === "k310") throw 0;
  if (x === "k311") throw 0;
  if (x === "k312") throw 0;
  if (x === "k313") throw 0;
  if (x === "k314") throw 0;
  if (x === "k315") throw 0;
  if (x === "k316") throw 0;
  if (x === "k317") throw 0;
  if (x === "k318") throw 0;
  if (x === "k319") throw 0;
  if (x === "k320") throw 0;
  if (x === "k321") throw 0;
  if (x === "k322") throw 0;
  if (x === "k323") throw 0;
  if (x === "k324") throw 0;
  if (x === "k325") throw 0;
  if (x === "k326") throw 0;
  if (x === "k327") throw 0;
  if (x === "k328") throw 0;
  if (x === "k329") throw 0;
  if (x === "k330") throw 0;
  if (x === "k331") throw 0;
  if (x === "k332") throw 0;
  if (x === "k333") throw 0;
  if (x === "k334") throw 0;
  if (x === "k335") throw 0;
  if (x === "k336") throw 0;
  if (x === "k337") throw 0;
  if (x === "k338") throw 0;
  if (x === "k339") throw 0;
  if (x === "k340") throw 0;
  if (x === "k341") throw 0;
  if (x === "k342") throw 0;
  if (x === "k343") throw 0;
  if (x === "k344") throw 0;
  if (x === "k345") throw 0;
  if (x === "k346") throw 0;
  if (x === "k347") throw 0;
  if (x === "k348") throw 0;
  if (x === "k349") throw 0;
  if (x === "k350") throw 0;
  if (x === "k351") throw 0;
  if (x === "k352") throw 0;
  if (x === "k353") throw 0;
  if (x === "k354") throw 0;
  if (x === "k355") throw 0;
  if (x === "k356") throw 0;
  if (x === "k357") throw 0;
  if (x === "k358") throw 0;
  if (x === "k359") throw 0;
  if (x === "k360") throw 0;
  if (x === "k361") throw 0;
  if (x === "k362") throw 0;
  if (x === "k363") throw 0;
  if (x === "k364") throw 0;
  if (x === "k365") throw 0;
  if (x === "k366") throw 0;
  if (x === "k367") throw 0;
  if (x === "k368") throw 0;
  if (x === "k369") throw 0;
  if (x === "k370") throw 0;
  if (x === "k371") throw 0;
  if (x === "k372") throw 0;
  if (x === "k373") throw 0;
  if (x === "k374") throw 0;
  if (x === "k375") throw 0;
  if (x === "k376") throw 0;
  if (x === "k377") throw 0;
  if (x === "k378") throw 0;
  if (x === "k379") throw 0;
  if (x === "k380") throw 0;
  if (x === "k381") throw 0;
  if (x === "k382") throw 0;
  if (x === "k383") throw 0;
  if (x === "k384") throw 0;
  if (x === "k385") throw 0;
  if (x === "k386") throw 0;
  if (x === "k387") throw 0;
  if (x === "k388") throw 0;
  if (x === "k389") throw 0;
  if (x === "k390") throw 0;
  if (x === "k391") throw 0;
  if (x === "k392") throw 0;
  if (x === "k393") throw 0;
  if (x === "k394") throw 0;
  if (x === "k395") throw 0;
  if (x === "k396") throw 0;
  if (x === "k397") throw 0;
  if (x === "k398") throw 0;
  if (x === "k399") throw 0;
  if (x === "k400") throw 0;
  if (x === "k401") throw 0;
  if (x === "k402") throw 0;
  if (x === "k403") throw 0;
  if (x === "k404") throw 0;
  if (x === "k405") throw 0;
  if (x === "k406") throw 0;
  if (x === "k407") throw 0;
  if (x === "k408") throw 0;
  if (x === "k409") throw 0;
  if (x === "k410") throw 0;
  if (x === "k411") throw 0;
  if (x === "k412") throw 0;
  if (x === "k413") throw 0;
  if (x === "k414") throw 0;
  if (x === "k415") throw 0;
  if (x === "k416") throw 0;
  if (x === "k417") throw 0;
  if (x === "k418") throw 0;
  if (x === "k419") throw 0;
  if (x === "k420") throw 0;
  if (x === "k421") throw 0;
  if (x === "k422") throw 0;
  if (x === "k423") throw 0;
  if (x === "k424") throw 0;
  if (x === "k425") throw 0;
  if (x === "k426") throw 0;
  if (x === "k427") throw 0;
  if (x === "k428") throw 0;
  if (x === "k429") throw 0;
  if (x === "k430") throw 0;
  if (x === "k431") throw 0;
  if (x === "k432") throw 0;
  if (x === "k433") throw 0;
  if (x === "k434") throw 0;
  if (x === "k435") throw 0;
  if (x === "k436") throw 0;
  if (x === "k437") throw 0;
  if (x === "k438") throw 0;
  if (x === "k439") throw 0;
  if (x === "k440") throw 0;
  if (x === "k441") throw 0;
  if (x === "k442") throw 0;
  if (x === "k443") throw 0;
  if (x === "k444") throw 0;
  if (x === "k445") throw 0;
  if (x === "k446") throw 0;
  if (x === "k447") throw 0;
  if (x === "k448") throw 0;
  if (x === "k449") throw 0;
  if (x === "k450") throw 0;
  if (x === "k451") throw 0;
  if (x === "k452") throw 0;
  if (x === "k453") throw 0;
  if (x === "k454") throw 0;
  if (x === "k455") throw 0;
  if (x === "k456") throw 0;
  if (x === "k457") throw 0;
  if (x === "k458") throw 0;
  if (x === "k459") throw 0;
  if (x === "k460") throw 0;
  if (x === "k461") throw 0;
  if (x === "k462") throw 0;
  if (x === "k463") throw 0;
  if (x === "k464") throw 0;
  if (x === "k465") throw 0;
  if (x === "k466") throw 0;
  if (x === "k467") throw 0;
  if (x === "k468") throw 0;
  if (x === "k469") throw 0;
  if (x === "k470") throw 0;
  if (x === "k471") throw 0;
  if (x === "k472") throw 0;
  if (x === "k473") throw 0;
  if (x === "k474") throw 0;
  if (x === "k475") throw 0;
  if (x === "k476") throw 0;
  if (x === "k477") throw 0;
  if (x === "k478") throw 0;
  if (x === "k479") throw 0;
  if (x === "k480") throw 0;
  if (x === "k481") throw 0;
  if (x === "k482") throw 0;
  if (x === "k483") throw 0;
  if (x === "k484") throw 0;
  if (x === "k485") throw 0;
  if (x === "k486") throw 0;
  if (x === "k487") throw 0;
  if (x === "k488") throw 0;
  if (x === "k489") throw 0;
  if (x === "k490") throw 0;
  if (x === "k491") throw 0;
  if (x === "k492") throw 0;
  if (x === "k493") throw 0;
  if (x === "k494") throw 0;
  if (x === "k495") throw 0;
  if (x === "k496") throw 0;
  if (x === "k497") throw 0;
  if (x === "k498") throw 0;
  if (x === "k499") throw 0;
  if (x === "k500") throw 0;
  if (x === "k501") throw 0;
  if (x === "k502") throw 0;
  if (x === "k503") throw 0;
  if (x === "k504") throw 0;
  if (x === "k505") throw 0;
  if (x === "k506") throw 0;
  if (x === "k507") throw 0;
  if (x === "k508") throw 0;
  if (x === "k509") throw 0;
  if (x === "k510") throw 0;
  if (x === "k511") throw 0;
  if (x === "k512") throw 0;
  if (x === "k513") throw 0;
  if (x === "k514") throw 0;
  if (x === "k515") throw 0;
  if (x === "k516") throw 0;
  if (x === "k517") throw 0;
  if (x === "k518") throw 0;
  if (x === "k519") throw 0;
  if (x === "k520") throw 0;
  if (x === "k521") throw 0;
  if (x === "k522") throw 0;
  if (x === "k523") throw 0;
  if (x === "k524") throw 0;
  if (x === "k525") throw 0;
  if (x === "k526") throw 0;
  if (x === "k527") throw 0;
  if (x === "k528") throw 0;
  if (x === "k529") throw 0;
  if (x === "k530") throw 0;
  if (x === "k531") throw 0;
  if (x === "k532") throw 0;
  if (x === "k533") throw 0;
  if (x === "k534") throw 0;
  if (x === "k535") throw 0;
  if (x === "k536") throw 0;
  if (x === "k537") throw 0;
  if (x === "k538") throw 0;
  if (x === "k539") throw 0;
  if (x === "k540") throw 0;
  if (x === "k541") throw 0;
  if (x === "k542") throw 0;
  if (x === "k543") throw 0;
  if (x === "k544") throw 0;
  if (x === "k545") throw 0;
  if (x === "k546") throw 0;
  if (x === "k547") throw 0;
  if (x === "k548") throw 0;
  if (x === "k549") throw 0;
  if (x === "k550") throw 0;
  if (x === "k551") throw 0;
  if (x === "k552") throw 0;
  if (x === "k553") throw 0;
  if (x === "k554") throw 0;
  if (x === "k555") throw 0;
  if (x === "k556") throw 0;
  if (x === "k557") throw 0;
  if (x === "k558") throw 0;
  if (x === "k559") throw 0;
  if (x === "k560") throw 0;
  if (x === "k561") throw 0;
  if (x === "k562") throw 0;
  if (x === "k563") throw 0;
  if (x === "k564") throw 0;
  if (x === "k565") throw 0;
  if (x === "k566") throw 0;
  if (x === "k567") throw 0;
  if (x === "k568") throw 0;
  if (x === "k569") throw 0;
  if (x === "k570") throw 0;
  if (x === "k571") throw 0;
  if (x === "k572") throw 0;
  if (x === "k573") throw 0;
  if (x === "k574") throw 0;
  if (x === "k575") throw 0;
  if (x === "k576") throw 0;
  if (x === "k577") throw 0;
  if (x === "k578") throw 0;
  if (x === "k579") throw 0;
  if (x === "k580") throw 0;
  if (x === "k581") throw 0;
  if (x === "k582") throw 0;
  if (x === "k583") throw 0;
  if (x === "k584") throw 0;
  if (x === "k585") throw 0;
  if (x === "k586") throw 0;
  if (x === "k587") throw 0;
  if (x === "k588") throw 0;
  if (x === "k589") throw 0;
  if (x === "k590") throw 0;
  if (x === "k591") throw 0;
  if (x === "k592") throw 0;
  if (x === "k593") throw 0;
  if (x === "k594") throw 0;
  if (x === "k595") throw 0;
  if (x === "k596") throw 0;
  if (x === "k597") throw 0;
  if (x === "k598") throw 0;
  if (x === "k599") throw 0;
  if (x === "k600") throw 0;
  if (x === "k601") throw 0;
  if (x === "k602") throw 0;
  if (x === "k603") throw 0;
  if (x === "k604") throw 0;
  if (x === "k605") throw 0;
  if (x === "k606") throw 0;
  if (x === "k607") throw 0;
  if (x === "k608") throw 0;
  if (x === "k609") throw 0;
  if (x === "k610") throw 0;
  if (x === "k611") throw 0;
  if (x === "k612") throw 0;
  if (x === "k613") throw 0;
  if (x === "k614") throw 0;
  if (x === "k615") throw 0;
  if (x === "k616") throw 0;
  if (x === "k617") throw 0;
  if (x === "k618") throw 0;
  if (x === "k619") throw 0;
  if (x === "k620") throw 0;
  if (x === "k621") throw 0;
  if (x === "k622") throw 0;
  if (x === "k623") throw 0;
  if (x === "k624") throw 0;
  if (x === "k625") throw 0;
  if (x === "k626") throw 0;
  if (x === "k627") throw 0;
  if (x === "k628") throw 0;
  if (x === "k629") throw 0;
  if (x === "k630") throw 0;
  if (x === "k631") throw 0;
  if (x === "k632") throw 0;
  if (x === "k633") throw 0;
  if (x === "k634") throw 0;
  if (x === "k635") throw 0;
  if (x === "k636") throw 0;
  if (x === "k637") throw 0;
  if (x === "k638") throw 0;
  if (x === "k639") throw 0;
  if (x === "k640") throw 0;
  if (x === "k641") throw 0;
  if (x === "k642") throw 0;
  if (x === "k643") throw 0;
  if (x === "k644") throw 0;
  if (x === "k645") throw 0;
  if (x === "k646") throw 0;
  if (x === "k647") throw 0;
  if (x === "k648") throw 0;
  if (x === "k649") throw 0;
  if (x === "k650") throw 0;
  if (x === "k651") throw 0;
  if (x === "k652") throw 0;
  if (x === "k653") throw 0;
  if (x === "k654") throw 0;
  if (x === "k655") throw 0;
  if (x === "k656") throw 0;
  if (x === "k657") throw 0;
  if (x === "k658") throw 0;
  if (x === "k659") throw 0;
  if (x === "k660") throw 0;
  if (x === "k661") throw 0;
  if (x === "k662") throw 0;
  if (x === "k663") throw 0;
  if (x === "k664") throw 0;
  if (x === "k665") throw 0;
  if (x === "k666") throw 0;
  if (x === "k667") throw 0;
  if (x === "k668") throw 0;
  if (x === "k669") throw 0;
  if (x === "k670") throw 0;
  if (x === "k671") throw 0;
  if (x === "k672") throw 0;
  if (x === "k673") throw 0;
  if (x === "k674") throw 0;
  if (x === "k675") throw 0;
  if (x === "k676") throw 0;
  if (x === "k677") throw 0;
  if (x === "k678") throw 0;
  if (x === "k679") throw 0;
  if (x === "k680") throw 0;
  if (x === "k681") throw 0;
  if (x === "k682") throw 0;
  if (x === "k683") throw 0;
  if (x === "k684") throw 0;
  if (x === "k685") throw 0;
  if (x === "k686") throw 0;
  if (x === "k687") throw 0;
  if (x === "k688") throw 0;
  if (x === "k689") throw 0;
  if (x === "k690") throw 0;
  if (x === "k691") throw 0;
  if (x === "k692") throw 0;
  if (x === "k693") throw 0;
  if (x === "k694") throw 0;
  if (x === "k695") throw 0;
  if (x === "k696") throw 0;
  if (x === "k697") throw 0;
  if (x === "k698") throw 0;
  if (x === "k699") throw 0;
  if (x === "k700") throw 0;
  if (x === "k701") throw 0;
  if (x === "k702") throw 0;
  if (x === "k703") throw 0;
  if (x === "k704") throw 0;
  if (x === "k705") throw 0;
  if (x === "k706") throw 0;
  if (x === "k707") throw 0;
  if (x === "k708") throw 0;
  if (x === "k709") throw 0;
  if (x === "k710") throw 0;
  if (x === "k711") throw 0;
  if (x === "k712") throw 0;
  if (x === "k713") throw 0;
  if (x === "k714") throw 0;
  if (x === "k715") throw 0;
  if (x === "k716") throw 0;
  if (x === "k717") throw 0;
  if (x === "k718") throw 0;
  if (x === "k719") throw 0;
  if (x === "k720") throw 0;
  if (x === "k721") throw 0;
  if (x === "k722") throw 0;
  if (x === "k723") throw 0;
  if (x === "k724") throw 0;
  if (x === "k725") throw 0;
  if (x === "k726") throw 0;
  if (x === "k727") throw 0;
  if (x === "k728") throw 0;
  if (x === "k729") throw 0;
  if (x === "k730") throw 0;
  if (x === "k731") throw 0;
  if (x === "k732") throw 0;
  if (x === "k733") throw 0;
  if (x === "k734") throw 0;
  if (x === "k735") throw 0;
  if (x === "k736") throw 0;
  if (x === "k737") throw 0;
  if (x === "k738") throw 0;
  if (x === "k739") throw 0;
  if (x === "k740") throw 0;
  if (x === "k741") throw 0;
  if (x === "k742") throw 0;
  if (x === "k743") throw 0;
  if (x === "k744") throw 0;
  if (x === "k745") throw 0;
  if (x === "k746") throw 0;
  if (x === "k747") throw 0;
  if (x === "k748") throw 0;
  if (x === "k749") throw 0;
  if (x === "k750") throw 0;
  if (x === "k751") throw 0;
  if (x === "k752") throw 0;
  if (x === "k753") throw 0;
  if (x === "k754") throw 0;
  if (x === "k755") throw 0;
  if (x === "k756") throw 0;
  if (x === "k757") throw 0;
  if (x === "k758") throw 0;
  if (x === "k759") throw 0;
  if (x === "k760") throw 0;
  if (x === "k761") throw 0;
  if (x === "k762") throw 0;
  if (x === "k763") throw 0;
  if (x === "k764") throw 0;
  if (x === "k765") throw 0;
  if (x === "k766") throw 0;
  if (x === "k767") throw 0;
  if (x === "k768") throw 0;
  if (x === "k769") throw 0;
  if (x === "k770") throw 0;
  if (x === "k771") throw 0;
  if (x === "k772") throw 0;
  if (x === "k773") throw 0;
  if (x === "k774") throw 0;
  if (x === "k775") throw 0;
  if (x === "k776") throw 0;
  if (x === "k777") throw 0;
  if (x === "k778") throw 0;
  if (x === "k779") throw 0;
  if (x === "k780") throw 0;
  if (x === "k781") throw 0;
  if (x === "k782") throw 0;
  if (x === "k783") throw 0;
  if (x === "k784") throw 0;
  if (x === "k785") throw 0;
  if (x === "k786") throw 0;
  if (x === "k787") throw 0;
  if (x === "k788") throw 0;
  if (x === "k789") throw 0;
  if (x === "k790") throw 0;
  if (x === "k791") throw 0;
  if (x === "k792") throw 0;
  if (x === "k793") throw 0;
  if (x === "k794") throw 0;
  if (x === "k795") throw 0;
  if (x === "k796") throw 0;
  if (x === "k797") throw 0;
  if (x === "k798") throw 0;
  return x;
}
"##;

/// 799 `if (x === "k<i>") throw 0;` guards narrow the 800-member union to
/// `"k799"`. The lane's time grows super-linearly: 50 guards 0.8 s, 200 13.1 s
/// for the four settings; at 800 every setting passes the row deadline.
///
/// What the lane gives:
/// - `last`: the checker answers `"k799"`; the lane took longer than 60s.
#[test]
#[ignore = "a long chain of equality guards narrows in time the checker takes"]
fn an_800_guard_narrowing_chain_answers() {
    let matrix = Matrix::new(NARROW_CHAIN_800);
    let failures = matrix.returns(&[("last", "\"k799\"")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// 800 returning guards.
const RETURN_CHAIN_800: &str = r##"export function chain(x: number) {
  if (x === 0) return "r0" as const;
  if (x === 1) return "r1" as const;
  if (x === 2) return "r2" as const;
  if (x === 3) return "r3" as const;
  if (x === 4) return "r4" as const;
  if (x === 5) return "r5" as const;
  if (x === 6) return "r6" as const;
  if (x === 7) return "r7" as const;
  if (x === 8) return "r8" as const;
  if (x === 9) return "r9" as const;
  if (x === 10) return "r10" as const;
  if (x === 11) return "r11" as const;
  if (x === 12) return "r12" as const;
  if (x === 13) return "r13" as const;
  if (x === 14) return "r14" as const;
  if (x === 15) return "r15" as const;
  if (x === 16) return "r16" as const;
  if (x === 17) return "r17" as const;
  if (x === 18) return "r18" as const;
  if (x === 19) return "r19" as const;
  if (x === 20) return "r20" as const;
  if (x === 21) return "r21" as const;
  if (x === 22) return "r22" as const;
  if (x === 23) return "r23" as const;
  if (x === 24) return "r24" as const;
  if (x === 25) return "r25" as const;
  if (x === 26) return "r26" as const;
  if (x === 27) return "r27" as const;
  if (x === 28) return "r28" as const;
  if (x === 29) return "r29" as const;
  if (x === 30) return "r30" as const;
  if (x === 31) return "r31" as const;
  if (x === 32) return "r32" as const;
  if (x === 33) return "r33" as const;
  if (x === 34) return "r34" as const;
  if (x === 35) return "r35" as const;
  if (x === 36) return "r36" as const;
  if (x === 37) return "r37" as const;
  if (x === 38) return "r38" as const;
  if (x === 39) return "r39" as const;
  if (x === 40) return "r40" as const;
  if (x === 41) return "r41" as const;
  if (x === 42) return "r42" as const;
  if (x === 43) return "r43" as const;
  if (x === 44) return "r44" as const;
  if (x === 45) return "r45" as const;
  if (x === 46) return "r46" as const;
  if (x === 47) return "r47" as const;
  if (x === 48) return "r48" as const;
  if (x === 49) return "r49" as const;
  if (x === 50) return "r50" as const;
  if (x === 51) return "r51" as const;
  if (x === 52) return "r52" as const;
  if (x === 53) return "r53" as const;
  if (x === 54) return "r54" as const;
  if (x === 55) return "r55" as const;
  if (x === 56) return "r56" as const;
  if (x === 57) return "r57" as const;
  if (x === 58) return "r58" as const;
  if (x === 59) return "r59" as const;
  if (x === 60) return "r60" as const;
  if (x === 61) return "r61" as const;
  if (x === 62) return "r62" as const;
  if (x === 63) return "r63" as const;
  if (x === 64) return "r64" as const;
  if (x === 65) return "r65" as const;
  if (x === 66) return "r66" as const;
  if (x === 67) return "r67" as const;
  if (x === 68) return "r68" as const;
  if (x === 69) return "r69" as const;
  if (x === 70) return "r70" as const;
  if (x === 71) return "r71" as const;
  if (x === 72) return "r72" as const;
  if (x === 73) return "r73" as const;
  if (x === 74) return "r74" as const;
  if (x === 75) return "r75" as const;
  if (x === 76) return "r76" as const;
  if (x === 77) return "r77" as const;
  if (x === 78) return "r78" as const;
  if (x === 79) return "r79" as const;
  if (x === 80) return "r80" as const;
  if (x === 81) return "r81" as const;
  if (x === 82) return "r82" as const;
  if (x === 83) return "r83" as const;
  if (x === 84) return "r84" as const;
  if (x === 85) return "r85" as const;
  if (x === 86) return "r86" as const;
  if (x === 87) return "r87" as const;
  if (x === 88) return "r88" as const;
  if (x === 89) return "r89" as const;
  if (x === 90) return "r90" as const;
  if (x === 91) return "r91" as const;
  if (x === 92) return "r92" as const;
  if (x === 93) return "r93" as const;
  if (x === 94) return "r94" as const;
  if (x === 95) return "r95" as const;
  if (x === 96) return "r96" as const;
  if (x === 97) return "r97" as const;
  if (x === 98) return "r98" as const;
  if (x === 99) return "r99" as const;
  if (x === 100) return "r100" as const;
  if (x === 101) return "r101" as const;
  if (x === 102) return "r102" as const;
  if (x === 103) return "r103" as const;
  if (x === 104) return "r104" as const;
  if (x === 105) return "r105" as const;
  if (x === 106) return "r106" as const;
  if (x === 107) return "r107" as const;
  if (x === 108) return "r108" as const;
  if (x === 109) return "r109" as const;
  if (x === 110) return "r110" as const;
  if (x === 111) return "r111" as const;
  if (x === 112) return "r112" as const;
  if (x === 113) return "r113" as const;
  if (x === 114) return "r114" as const;
  if (x === 115) return "r115" as const;
  if (x === 116) return "r116" as const;
  if (x === 117) return "r117" as const;
  if (x === 118) return "r118" as const;
  if (x === 119) return "r119" as const;
  if (x === 120) return "r120" as const;
  if (x === 121) return "r121" as const;
  if (x === 122) return "r122" as const;
  if (x === 123) return "r123" as const;
  if (x === 124) return "r124" as const;
  if (x === 125) return "r125" as const;
  if (x === 126) return "r126" as const;
  if (x === 127) return "r127" as const;
  if (x === 128) return "r128" as const;
  if (x === 129) return "r129" as const;
  if (x === 130) return "r130" as const;
  if (x === 131) return "r131" as const;
  if (x === 132) return "r132" as const;
  if (x === 133) return "r133" as const;
  if (x === 134) return "r134" as const;
  if (x === 135) return "r135" as const;
  if (x === 136) return "r136" as const;
  if (x === 137) return "r137" as const;
  if (x === 138) return "r138" as const;
  if (x === 139) return "r139" as const;
  if (x === 140) return "r140" as const;
  if (x === 141) return "r141" as const;
  if (x === 142) return "r142" as const;
  if (x === 143) return "r143" as const;
  if (x === 144) return "r144" as const;
  if (x === 145) return "r145" as const;
  if (x === 146) return "r146" as const;
  if (x === 147) return "r147" as const;
  if (x === 148) return "r148" as const;
  if (x === 149) return "r149" as const;
  if (x === 150) return "r150" as const;
  if (x === 151) return "r151" as const;
  if (x === 152) return "r152" as const;
  if (x === 153) return "r153" as const;
  if (x === 154) return "r154" as const;
  if (x === 155) return "r155" as const;
  if (x === 156) return "r156" as const;
  if (x === 157) return "r157" as const;
  if (x === 158) return "r158" as const;
  if (x === 159) return "r159" as const;
  if (x === 160) return "r160" as const;
  if (x === 161) return "r161" as const;
  if (x === 162) return "r162" as const;
  if (x === 163) return "r163" as const;
  if (x === 164) return "r164" as const;
  if (x === 165) return "r165" as const;
  if (x === 166) return "r166" as const;
  if (x === 167) return "r167" as const;
  if (x === 168) return "r168" as const;
  if (x === 169) return "r169" as const;
  if (x === 170) return "r170" as const;
  if (x === 171) return "r171" as const;
  if (x === 172) return "r172" as const;
  if (x === 173) return "r173" as const;
  if (x === 174) return "r174" as const;
  if (x === 175) return "r175" as const;
  if (x === 176) return "r176" as const;
  if (x === 177) return "r177" as const;
  if (x === 178) return "r178" as const;
  if (x === 179) return "r179" as const;
  if (x === 180) return "r180" as const;
  if (x === 181) return "r181" as const;
  if (x === 182) return "r182" as const;
  if (x === 183) return "r183" as const;
  if (x === 184) return "r184" as const;
  if (x === 185) return "r185" as const;
  if (x === 186) return "r186" as const;
  if (x === 187) return "r187" as const;
  if (x === 188) return "r188" as const;
  if (x === 189) return "r189" as const;
  if (x === 190) return "r190" as const;
  if (x === 191) return "r191" as const;
  if (x === 192) return "r192" as const;
  if (x === 193) return "r193" as const;
  if (x === 194) return "r194" as const;
  if (x === 195) return "r195" as const;
  if (x === 196) return "r196" as const;
  if (x === 197) return "r197" as const;
  if (x === 198) return "r198" as const;
  if (x === 199) return "r199" as const;
  if (x === 200) return "r200" as const;
  if (x === 201) return "r201" as const;
  if (x === 202) return "r202" as const;
  if (x === 203) return "r203" as const;
  if (x === 204) return "r204" as const;
  if (x === 205) return "r205" as const;
  if (x === 206) return "r206" as const;
  if (x === 207) return "r207" as const;
  if (x === 208) return "r208" as const;
  if (x === 209) return "r209" as const;
  if (x === 210) return "r210" as const;
  if (x === 211) return "r211" as const;
  if (x === 212) return "r212" as const;
  if (x === 213) return "r213" as const;
  if (x === 214) return "r214" as const;
  if (x === 215) return "r215" as const;
  if (x === 216) return "r216" as const;
  if (x === 217) return "r217" as const;
  if (x === 218) return "r218" as const;
  if (x === 219) return "r219" as const;
  if (x === 220) return "r220" as const;
  if (x === 221) return "r221" as const;
  if (x === 222) return "r222" as const;
  if (x === 223) return "r223" as const;
  if (x === 224) return "r224" as const;
  if (x === 225) return "r225" as const;
  if (x === 226) return "r226" as const;
  if (x === 227) return "r227" as const;
  if (x === 228) return "r228" as const;
  if (x === 229) return "r229" as const;
  if (x === 230) return "r230" as const;
  if (x === 231) return "r231" as const;
  if (x === 232) return "r232" as const;
  if (x === 233) return "r233" as const;
  if (x === 234) return "r234" as const;
  if (x === 235) return "r235" as const;
  if (x === 236) return "r236" as const;
  if (x === 237) return "r237" as const;
  if (x === 238) return "r238" as const;
  if (x === 239) return "r239" as const;
  if (x === 240) return "r240" as const;
  if (x === 241) return "r241" as const;
  if (x === 242) return "r242" as const;
  if (x === 243) return "r243" as const;
  if (x === 244) return "r244" as const;
  if (x === 245) return "r245" as const;
  if (x === 246) return "r246" as const;
  if (x === 247) return "r247" as const;
  if (x === 248) return "r248" as const;
  if (x === 249) return "r249" as const;
  if (x === 250) return "r250" as const;
  if (x === 251) return "r251" as const;
  if (x === 252) return "r252" as const;
  if (x === 253) return "r253" as const;
  if (x === 254) return "r254" as const;
  if (x === 255) return "r255" as const;
  if (x === 256) return "r256" as const;
  if (x === 257) return "r257" as const;
  if (x === 258) return "r258" as const;
  if (x === 259) return "r259" as const;
  if (x === 260) return "r260" as const;
  if (x === 261) return "r261" as const;
  if (x === 262) return "r262" as const;
  if (x === 263) return "r263" as const;
  if (x === 264) return "r264" as const;
  if (x === 265) return "r265" as const;
  if (x === 266) return "r266" as const;
  if (x === 267) return "r267" as const;
  if (x === 268) return "r268" as const;
  if (x === 269) return "r269" as const;
  if (x === 270) return "r270" as const;
  if (x === 271) return "r271" as const;
  if (x === 272) return "r272" as const;
  if (x === 273) return "r273" as const;
  if (x === 274) return "r274" as const;
  if (x === 275) return "r275" as const;
  if (x === 276) return "r276" as const;
  if (x === 277) return "r277" as const;
  if (x === 278) return "r278" as const;
  if (x === 279) return "r279" as const;
  if (x === 280) return "r280" as const;
  if (x === 281) return "r281" as const;
  if (x === 282) return "r282" as const;
  if (x === 283) return "r283" as const;
  if (x === 284) return "r284" as const;
  if (x === 285) return "r285" as const;
  if (x === 286) return "r286" as const;
  if (x === 287) return "r287" as const;
  if (x === 288) return "r288" as const;
  if (x === 289) return "r289" as const;
  if (x === 290) return "r290" as const;
  if (x === 291) return "r291" as const;
  if (x === 292) return "r292" as const;
  if (x === 293) return "r293" as const;
  if (x === 294) return "r294" as const;
  if (x === 295) return "r295" as const;
  if (x === 296) return "r296" as const;
  if (x === 297) return "r297" as const;
  if (x === 298) return "r298" as const;
  if (x === 299) return "r299" as const;
  if (x === 300) return "r300" as const;
  if (x === 301) return "r301" as const;
  if (x === 302) return "r302" as const;
  if (x === 303) return "r303" as const;
  if (x === 304) return "r304" as const;
  if (x === 305) return "r305" as const;
  if (x === 306) return "r306" as const;
  if (x === 307) return "r307" as const;
  if (x === 308) return "r308" as const;
  if (x === 309) return "r309" as const;
  if (x === 310) return "r310" as const;
  if (x === 311) return "r311" as const;
  if (x === 312) return "r312" as const;
  if (x === 313) return "r313" as const;
  if (x === 314) return "r314" as const;
  if (x === 315) return "r315" as const;
  if (x === 316) return "r316" as const;
  if (x === 317) return "r317" as const;
  if (x === 318) return "r318" as const;
  if (x === 319) return "r319" as const;
  if (x === 320) return "r320" as const;
  if (x === 321) return "r321" as const;
  if (x === 322) return "r322" as const;
  if (x === 323) return "r323" as const;
  if (x === 324) return "r324" as const;
  if (x === 325) return "r325" as const;
  if (x === 326) return "r326" as const;
  if (x === 327) return "r327" as const;
  if (x === 328) return "r328" as const;
  if (x === 329) return "r329" as const;
  if (x === 330) return "r330" as const;
  if (x === 331) return "r331" as const;
  if (x === 332) return "r332" as const;
  if (x === 333) return "r333" as const;
  if (x === 334) return "r334" as const;
  if (x === 335) return "r335" as const;
  if (x === 336) return "r336" as const;
  if (x === 337) return "r337" as const;
  if (x === 338) return "r338" as const;
  if (x === 339) return "r339" as const;
  if (x === 340) return "r340" as const;
  if (x === 341) return "r341" as const;
  if (x === 342) return "r342" as const;
  if (x === 343) return "r343" as const;
  if (x === 344) return "r344" as const;
  if (x === 345) return "r345" as const;
  if (x === 346) return "r346" as const;
  if (x === 347) return "r347" as const;
  if (x === 348) return "r348" as const;
  if (x === 349) return "r349" as const;
  if (x === 350) return "r350" as const;
  if (x === 351) return "r351" as const;
  if (x === 352) return "r352" as const;
  if (x === 353) return "r353" as const;
  if (x === 354) return "r354" as const;
  if (x === 355) return "r355" as const;
  if (x === 356) return "r356" as const;
  if (x === 357) return "r357" as const;
  if (x === 358) return "r358" as const;
  if (x === 359) return "r359" as const;
  if (x === 360) return "r360" as const;
  if (x === 361) return "r361" as const;
  if (x === 362) return "r362" as const;
  if (x === 363) return "r363" as const;
  if (x === 364) return "r364" as const;
  if (x === 365) return "r365" as const;
  if (x === 366) return "r366" as const;
  if (x === 367) return "r367" as const;
  if (x === 368) return "r368" as const;
  if (x === 369) return "r369" as const;
  if (x === 370) return "r370" as const;
  if (x === 371) return "r371" as const;
  if (x === 372) return "r372" as const;
  if (x === 373) return "r373" as const;
  if (x === 374) return "r374" as const;
  if (x === 375) return "r375" as const;
  if (x === 376) return "r376" as const;
  if (x === 377) return "r377" as const;
  if (x === 378) return "r378" as const;
  if (x === 379) return "r379" as const;
  if (x === 380) return "r380" as const;
  if (x === 381) return "r381" as const;
  if (x === 382) return "r382" as const;
  if (x === 383) return "r383" as const;
  if (x === 384) return "r384" as const;
  if (x === 385) return "r385" as const;
  if (x === 386) return "r386" as const;
  if (x === 387) return "r387" as const;
  if (x === 388) return "r388" as const;
  if (x === 389) return "r389" as const;
  if (x === 390) return "r390" as const;
  if (x === 391) return "r391" as const;
  if (x === 392) return "r392" as const;
  if (x === 393) return "r393" as const;
  if (x === 394) return "r394" as const;
  if (x === 395) return "r395" as const;
  if (x === 396) return "r396" as const;
  if (x === 397) return "r397" as const;
  if (x === 398) return "r398" as const;
  if (x === 399) return "r399" as const;
  if (x === 400) return "r400" as const;
  if (x === 401) return "r401" as const;
  if (x === 402) return "r402" as const;
  if (x === 403) return "r403" as const;
  if (x === 404) return "r404" as const;
  if (x === 405) return "r405" as const;
  if (x === 406) return "r406" as const;
  if (x === 407) return "r407" as const;
  if (x === 408) return "r408" as const;
  if (x === 409) return "r409" as const;
  if (x === 410) return "r410" as const;
  if (x === 411) return "r411" as const;
  if (x === 412) return "r412" as const;
  if (x === 413) return "r413" as const;
  if (x === 414) return "r414" as const;
  if (x === 415) return "r415" as const;
  if (x === 416) return "r416" as const;
  if (x === 417) return "r417" as const;
  if (x === 418) return "r418" as const;
  if (x === 419) return "r419" as const;
  if (x === 420) return "r420" as const;
  if (x === 421) return "r421" as const;
  if (x === 422) return "r422" as const;
  if (x === 423) return "r423" as const;
  if (x === 424) return "r424" as const;
  if (x === 425) return "r425" as const;
  if (x === 426) return "r426" as const;
  if (x === 427) return "r427" as const;
  if (x === 428) return "r428" as const;
  if (x === 429) return "r429" as const;
  if (x === 430) return "r430" as const;
  if (x === 431) return "r431" as const;
  if (x === 432) return "r432" as const;
  if (x === 433) return "r433" as const;
  if (x === 434) return "r434" as const;
  if (x === 435) return "r435" as const;
  if (x === 436) return "r436" as const;
  if (x === 437) return "r437" as const;
  if (x === 438) return "r438" as const;
  if (x === 439) return "r439" as const;
  if (x === 440) return "r440" as const;
  if (x === 441) return "r441" as const;
  if (x === 442) return "r442" as const;
  if (x === 443) return "r443" as const;
  if (x === 444) return "r444" as const;
  if (x === 445) return "r445" as const;
  if (x === 446) return "r446" as const;
  if (x === 447) return "r447" as const;
  if (x === 448) return "r448" as const;
  if (x === 449) return "r449" as const;
  if (x === 450) return "r450" as const;
  if (x === 451) return "r451" as const;
  if (x === 452) return "r452" as const;
  if (x === 453) return "r453" as const;
  if (x === 454) return "r454" as const;
  if (x === 455) return "r455" as const;
  if (x === 456) return "r456" as const;
  if (x === 457) return "r457" as const;
  if (x === 458) return "r458" as const;
  if (x === 459) return "r459" as const;
  if (x === 460) return "r460" as const;
  if (x === 461) return "r461" as const;
  if (x === 462) return "r462" as const;
  if (x === 463) return "r463" as const;
  if (x === 464) return "r464" as const;
  if (x === 465) return "r465" as const;
  if (x === 466) return "r466" as const;
  if (x === 467) return "r467" as const;
  if (x === 468) return "r468" as const;
  if (x === 469) return "r469" as const;
  if (x === 470) return "r470" as const;
  if (x === 471) return "r471" as const;
  if (x === 472) return "r472" as const;
  if (x === 473) return "r473" as const;
  if (x === 474) return "r474" as const;
  if (x === 475) return "r475" as const;
  if (x === 476) return "r476" as const;
  if (x === 477) return "r477" as const;
  if (x === 478) return "r478" as const;
  if (x === 479) return "r479" as const;
  if (x === 480) return "r480" as const;
  if (x === 481) return "r481" as const;
  if (x === 482) return "r482" as const;
  if (x === 483) return "r483" as const;
  if (x === 484) return "r484" as const;
  if (x === 485) return "r485" as const;
  if (x === 486) return "r486" as const;
  if (x === 487) return "r487" as const;
  if (x === 488) return "r488" as const;
  if (x === 489) return "r489" as const;
  if (x === 490) return "r490" as const;
  if (x === 491) return "r491" as const;
  if (x === 492) return "r492" as const;
  if (x === 493) return "r493" as const;
  if (x === 494) return "r494" as const;
  if (x === 495) return "r495" as const;
  if (x === 496) return "r496" as const;
  if (x === 497) return "r497" as const;
  if (x === 498) return "r498" as const;
  if (x === 499) return "r499" as const;
  if (x === 500) return "r500" as const;
  if (x === 501) return "r501" as const;
  if (x === 502) return "r502" as const;
  if (x === 503) return "r503" as const;
  if (x === 504) return "r504" as const;
  if (x === 505) return "r505" as const;
  if (x === 506) return "r506" as const;
  if (x === 507) return "r507" as const;
  if (x === 508) return "r508" as const;
  if (x === 509) return "r509" as const;
  if (x === 510) return "r510" as const;
  if (x === 511) return "r511" as const;
  if (x === 512) return "r512" as const;
  if (x === 513) return "r513" as const;
  if (x === 514) return "r514" as const;
  if (x === 515) return "r515" as const;
  if (x === 516) return "r516" as const;
  if (x === 517) return "r517" as const;
  if (x === 518) return "r518" as const;
  if (x === 519) return "r519" as const;
  if (x === 520) return "r520" as const;
  if (x === 521) return "r521" as const;
  if (x === 522) return "r522" as const;
  if (x === 523) return "r523" as const;
  if (x === 524) return "r524" as const;
  if (x === 525) return "r525" as const;
  if (x === 526) return "r526" as const;
  if (x === 527) return "r527" as const;
  if (x === 528) return "r528" as const;
  if (x === 529) return "r529" as const;
  if (x === 530) return "r530" as const;
  if (x === 531) return "r531" as const;
  if (x === 532) return "r532" as const;
  if (x === 533) return "r533" as const;
  if (x === 534) return "r534" as const;
  if (x === 535) return "r535" as const;
  if (x === 536) return "r536" as const;
  if (x === 537) return "r537" as const;
  if (x === 538) return "r538" as const;
  if (x === 539) return "r539" as const;
  if (x === 540) return "r540" as const;
  if (x === 541) return "r541" as const;
  if (x === 542) return "r542" as const;
  if (x === 543) return "r543" as const;
  if (x === 544) return "r544" as const;
  if (x === 545) return "r545" as const;
  if (x === 546) return "r546" as const;
  if (x === 547) return "r547" as const;
  if (x === 548) return "r548" as const;
  if (x === 549) return "r549" as const;
  if (x === 550) return "r550" as const;
  if (x === 551) return "r551" as const;
  if (x === 552) return "r552" as const;
  if (x === 553) return "r553" as const;
  if (x === 554) return "r554" as const;
  if (x === 555) return "r555" as const;
  if (x === 556) return "r556" as const;
  if (x === 557) return "r557" as const;
  if (x === 558) return "r558" as const;
  if (x === 559) return "r559" as const;
  if (x === 560) return "r560" as const;
  if (x === 561) return "r561" as const;
  if (x === 562) return "r562" as const;
  if (x === 563) return "r563" as const;
  if (x === 564) return "r564" as const;
  if (x === 565) return "r565" as const;
  if (x === 566) return "r566" as const;
  if (x === 567) return "r567" as const;
  if (x === 568) return "r568" as const;
  if (x === 569) return "r569" as const;
  if (x === 570) return "r570" as const;
  if (x === 571) return "r571" as const;
  if (x === 572) return "r572" as const;
  if (x === 573) return "r573" as const;
  if (x === 574) return "r574" as const;
  if (x === 575) return "r575" as const;
  if (x === 576) return "r576" as const;
  if (x === 577) return "r577" as const;
  if (x === 578) return "r578" as const;
  if (x === 579) return "r579" as const;
  if (x === 580) return "r580" as const;
  if (x === 581) return "r581" as const;
  if (x === 582) return "r582" as const;
  if (x === 583) return "r583" as const;
  if (x === 584) return "r584" as const;
  if (x === 585) return "r585" as const;
  if (x === 586) return "r586" as const;
  if (x === 587) return "r587" as const;
  if (x === 588) return "r588" as const;
  if (x === 589) return "r589" as const;
  if (x === 590) return "r590" as const;
  if (x === 591) return "r591" as const;
  if (x === 592) return "r592" as const;
  if (x === 593) return "r593" as const;
  if (x === 594) return "r594" as const;
  if (x === 595) return "r595" as const;
  if (x === 596) return "r596" as const;
  if (x === 597) return "r597" as const;
  if (x === 598) return "r598" as const;
  if (x === 599) return "r599" as const;
  if (x === 600) return "r600" as const;
  if (x === 601) return "r601" as const;
  if (x === 602) return "r602" as const;
  if (x === 603) return "r603" as const;
  if (x === 604) return "r604" as const;
  if (x === 605) return "r605" as const;
  if (x === 606) return "r606" as const;
  if (x === 607) return "r607" as const;
  if (x === 608) return "r608" as const;
  if (x === 609) return "r609" as const;
  if (x === 610) return "r610" as const;
  if (x === 611) return "r611" as const;
  if (x === 612) return "r612" as const;
  if (x === 613) return "r613" as const;
  if (x === 614) return "r614" as const;
  if (x === 615) return "r615" as const;
  if (x === 616) return "r616" as const;
  if (x === 617) return "r617" as const;
  if (x === 618) return "r618" as const;
  if (x === 619) return "r619" as const;
  if (x === 620) return "r620" as const;
  if (x === 621) return "r621" as const;
  if (x === 622) return "r622" as const;
  if (x === 623) return "r623" as const;
  if (x === 624) return "r624" as const;
  if (x === 625) return "r625" as const;
  if (x === 626) return "r626" as const;
  if (x === 627) return "r627" as const;
  if (x === 628) return "r628" as const;
  if (x === 629) return "r629" as const;
  if (x === 630) return "r630" as const;
  if (x === 631) return "r631" as const;
  if (x === 632) return "r632" as const;
  if (x === 633) return "r633" as const;
  if (x === 634) return "r634" as const;
  if (x === 635) return "r635" as const;
  if (x === 636) return "r636" as const;
  if (x === 637) return "r637" as const;
  if (x === 638) return "r638" as const;
  if (x === 639) return "r639" as const;
  if (x === 640) return "r640" as const;
  if (x === 641) return "r641" as const;
  if (x === 642) return "r642" as const;
  if (x === 643) return "r643" as const;
  if (x === 644) return "r644" as const;
  if (x === 645) return "r645" as const;
  if (x === 646) return "r646" as const;
  if (x === 647) return "r647" as const;
  if (x === 648) return "r648" as const;
  if (x === 649) return "r649" as const;
  if (x === 650) return "r650" as const;
  if (x === 651) return "r651" as const;
  if (x === 652) return "r652" as const;
  if (x === 653) return "r653" as const;
  if (x === 654) return "r654" as const;
  if (x === 655) return "r655" as const;
  if (x === 656) return "r656" as const;
  if (x === 657) return "r657" as const;
  if (x === 658) return "r658" as const;
  if (x === 659) return "r659" as const;
  if (x === 660) return "r660" as const;
  if (x === 661) return "r661" as const;
  if (x === 662) return "r662" as const;
  if (x === 663) return "r663" as const;
  if (x === 664) return "r664" as const;
  if (x === 665) return "r665" as const;
  if (x === 666) return "r666" as const;
  if (x === 667) return "r667" as const;
  if (x === 668) return "r668" as const;
  if (x === 669) return "r669" as const;
  if (x === 670) return "r670" as const;
  if (x === 671) return "r671" as const;
  if (x === 672) return "r672" as const;
  if (x === 673) return "r673" as const;
  if (x === 674) return "r674" as const;
  if (x === 675) return "r675" as const;
  if (x === 676) return "r676" as const;
  if (x === 677) return "r677" as const;
  if (x === 678) return "r678" as const;
  if (x === 679) return "r679" as const;
  if (x === 680) return "r680" as const;
  if (x === 681) return "r681" as const;
  if (x === 682) return "r682" as const;
  if (x === 683) return "r683" as const;
  if (x === 684) return "r684" as const;
  if (x === 685) return "r685" as const;
  if (x === 686) return "r686" as const;
  if (x === 687) return "r687" as const;
  if (x === 688) return "r688" as const;
  if (x === 689) return "r689" as const;
  if (x === 690) return "r690" as const;
  if (x === 691) return "r691" as const;
  if (x === 692) return "r692" as const;
  if (x === 693) return "r693" as const;
  if (x === 694) return "r694" as const;
  if (x === 695) return "r695" as const;
  if (x === 696) return "r696" as const;
  if (x === 697) return "r697" as const;
  if (x === 698) return "r698" as const;
  if (x === 699) return "r699" as const;
  if (x === 700) return "r700" as const;
  if (x === 701) return "r701" as const;
  if (x === 702) return "r702" as const;
  if (x === 703) return "r703" as const;
  if (x === 704) return "r704" as const;
  if (x === 705) return "r705" as const;
  if (x === 706) return "r706" as const;
  if (x === 707) return "r707" as const;
  if (x === 708) return "r708" as const;
  if (x === 709) return "r709" as const;
  if (x === 710) return "r710" as const;
  if (x === 711) return "r711" as const;
  if (x === 712) return "r712" as const;
  if (x === 713) return "r713" as const;
  if (x === 714) return "r714" as const;
  if (x === 715) return "r715" as const;
  if (x === 716) return "r716" as const;
  if (x === 717) return "r717" as const;
  if (x === 718) return "r718" as const;
  if (x === 719) return "r719" as const;
  if (x === 720) return "r720" as const;
  if (x === 721) return "r721" as const;
  if (x === 722) return "r722" as const;
  if (x === 723) return "r723" as const;
  if (x === 724) return "r724" as const;
  if (x === 725) return "r725" as const;
  if (x === 726) return "r726" as const;
  if (x === 727) return "r727" as const;
  if (x === 728) return "r728" as const;
  if (x === 729) return "r729" as const;
  if (x === 730) return "r730" as const;
  if (x === 731) return "r731" as const;
  if (x === 732) return "r732" as const;
  if (x === 733) return "r733" as const;
  if (x === 734) return "r734" as const;
  if (x === 735) return "r735" as const;
  if (x === 736) return "r736" as const;
  if (x === 737) return "r737" as const;
  if (x === 738) return "r738" as const;
  if (x === 739) return "r739" as const;
  if (x === 740) return "r740" as const;
  if (x === 741) return "r741" as const;
  if (x === 742) return "r742" as const;
  if (x === 743) return "r743" as const;
  if (x === 744) return "r744" as const;
  if (x === 745) return "r745" as const;
  if (x === 746) return "r746" as const;
  if (x === 747) return "r747" as const;
  if (x === 748) return "r748" as const;
  if (x === 749) return "r749" as const;
  if (x === 750) return "r750" as const;
  if (x === 751) return "r751" as const;
  if (x === 752) return "r752" as const;
  if (x === 753) return "r753" as const;
  if (x === 754) return "r754" as const;
  if (x === 755) return "r755" as const;
  if (x === 756) return "r756" as const;
  if (x === 757) return "r757" as const;
  if (x === 758) return "r758" as const;
  if (x === 759) return "r759" as const;
  if (x === 760) return "r760" as const;
  if (x === 761) return "r761" as const;
  if (x === 762) return "r762" as const;
  if (x === 763) return "r763" as const;
  if (x === 764) return "r764" as const;
  if (x === 765) return "r765" as const;
  if (x === 766) return "r766" as const;
  if (x === 767) return "r767" as const;
  if (x === 768) return "r768" as const;
  if (x === 769) return "r769" as const;
  if (x === 770) return "r770" as const;
  if (x === 771) return "r771" as const;
  if (x === 772) return "r772" as const;
  if (x === 773) return "r773" as const;
  if (x === 774) return "r774" as const;
  if (x === 775) return "r775" as const;
  if (x === 776) return "r776" as const;
  if (x === 777) return "r777" as const;
  if (x === 778) return "r778" as const;
  if (x === 779) return "r779" as const;
  if (x === 780) return "r780" as const;
  if (x === 781) return "r781" as const;
  if (x === 782) return "r782" as const;
  if (x === 783) return "r783" as const;
  if (x === 784) return "r784" as const;
  if (x === 785) return "r785" as const;
  if (x === 786) return "r786" as const;
  if (x === 787) return "r787" as const;
  if (x === 788) return "r788" as const;
  if (x === 789) return "r789" as const;
  if (x === 790) return "r790" as const;
  if (x === 791) return "r791" as const;
  if (x === 792) return "r792" as const;
  if (x === 793) return "r793" as const;
  if (x === 794) return "r794" as const;
  if (x === 795) return "r795" as const;
  if (x === 796) return "r796" as const;
  if (x === 797) return "r797" as const;
  if (x === 798) return "r798" as const;
  if (x === 799) return "r799" as const;
  return "none" as const;
}
"##;

/// A function with 800 `if (x === i) return "r<i>" as const;` statements
/// returns the union of the 801 literals. The lane fails with
/// `Budget(WorkBudgetExceeded)` (200 guards answer in 0.4 s).
///
/// What the lane gives:
/// - `chain`: the checker answers `"none" | "r0" | "r1" | "r10" | "r100" |
///   "r101" | "r102" | "r103" | "r104" | "r105" | "r106" | "r107" | "r108" |
///   "r109" | "r11" | "r110" | "r111" | "r112" | "r113" | "r114" | "r115" |
///   "r116" | "r117" | "r118" | "r119" | "r12" | "r120" | "r121" | "r122" |
///   "r123" | "r124" | "r125" | "r126" | "r127" | "r128" | "r129" | "r13" |
///   "r130" | "r131" | "r132" | "r133" | "r134" | "r135" | "r136" | "r137" |
///   "r138" | "r139" | "r14" | "r140" | "r141" | "r142" | "r143" | "r144" |
///   "r145" | "r146" | "r147" | "r148" | "r149" | "r15" | "r150" | "r151" |
///   "r152" | "r153" | "r154" | "r155" | "r156" | "r157" | "r158" | "r159" |
///   "r16" | "r160" | "r161" | "r162" | "r163" | "r164" | "r165" | "r166" |
///   "r167" | "r168" | "r169" | "r17" | "r170" | "r171" | "r172" | "r173" |
///   "r174" | "r175" | "r176" | "r177" | "r178" | "r179" | "r18" | "r180" |
///   "r181" | "r182" | "r183" | "r184" | "r185" | "r186" | "r187" | "r188" |
///   "r189" | "r19" | "r190" | "r191" | "r192" | "r193" | "r194" | "r195" |
///   "r196" | "r197" | "r198" | "r199" | "r2" | "r20" | "r200" | "r201" |
///   "r202" | "r203" | "r204" | "r205" | "r206" | "r207" | "r208" | "r209" |
///   "r21" | "r210" | "r211" | "r212" | "r213" | "r214" | "r215" | "r216" |
///   "r217" | "r218" | "r219" | "r22" | "r220" | "r221" | "r222" | "r223" |
///   "r224" | "r225" | "r226" | "r227" | "r228" | "r229" | "r23" | "r230" |
///   "r231" | "r232" | "r233" | "r234" | "r235" | "r236" | "r237" | "r238" |
///   "r239" | "r24" | "r240" | "r241" | "r242" | "r243" | "r244" | "r245" |
///   "r246" | "r247" | "r248" | "r249" | "r25" | "r250" | "r251" | "r252" |
///   "r253" | "r254" | "r255" | "r256" | "r257" | "r258" | "r259" | "r26" |
///   "r260" | "r261" | "r262" | "r263" | "r264" | "r265" | "r266" | "r267" |
///   "r268" | "r269" | "r27" | "r270" | "r271" | "r272" | "r273" | "r274" |
///   "r275" | "r276" | "r277" | "r278" | "r279" | "r28" | "r280" | "r281" |
///   "r282" | "r283" | "r284" | "r285" | "r286" | "r287" | "r288" | "r289" |
///   "r29" | "r290" | "r291" | "r292" | "r293" | "r294" | "r295" | "r296" |
///   "r297" | "r298" | "r299" | "r3" | "r30" | "r300" | "r301" | "r302" |
///   "r303" | "r304" | "r305" | "r306" | "r307" | "r308" | "r309" | "r31" |
///   "r310" | "r311" | "r312" | "r313" | "r314" | "r315" | "r316" | "r317" |
///   "r318" | "r319" | "r32" | "r320" | "r321" | "r322" | "r323" | "r324" |
///   "r325" | "r326" | "r327" | "r328" | "r329" | "r33" | "r330" | "r331" |
///   "r332" | "r333" | "r334" | "r335" | "r336" | "r337" | "r338" | "r339" |
///   "r34" | "r340" | "r341" | "r342" | "r343" | "r344" | "r345" | "r346" |
///   "r347" | "r348" | "r349" | "r35" | "r350" | "r351" | "r352" | "r353" |
///   "r354" | "r355" | "r356" | "r357" | "r358" | "r359" | "r36" | "r360" |
///   "r361" | "r362" | "r363" | "r364" | "r365" | "r366" | "r367" | "r368" |
///   "r369" | "r37" | "r370" | "r371" | "r372" | "r373" | "r374" | "r375" |
///   "r376" | "r377" | "r378" | "r379" | "r38" | "r380" | "r381" | "r382" |
///   "r383" | "r384" | "r385" | "r386" | "r387" | "r388" | "r389" | "r39" |
///   "r390" | "r391" | "r392" | "r393" | "r394" | "r395" | "r396" | "r397" |
///   "r398" | "r399" | "r4" | "r40" | "r400" | "r401" | "r402" | "r403" |
///   "r404" | "r405" | "r406" | "r407" | "r408" | "r409" | "r41" | "r410" |
///   "r411" | "r412" | "r413" | "r414" | "r415" | "r416" | "r417" | "r418" |
///   "r419" | "r42" | "r420" | "r421" | "r422" | "r423" | "r424" | "r425" |
///   "r426" | "r427" | "r428" | "r429" | "r43" | "r430" | "r431" | "r432" |
///   "r433" | "r434" | "r435" | "r436" | "r437" | "r438" | "r439" | "r44" |
///   "r440" | "r441" | "r442" | "r443" | "r444" | "r445" | "r446" | "r447" |
///   "r448" | "r449" | "r45" | "r450" | "r451" | "r452" | "r453" | "r454" |
///   "r455" | "r456" | "r457" | "r458" | "r459" | "r46" | "r460" | "r461" |
///   "r462" | "r463" | "r464" | "r465" | "r466" | "r467" | "r468" | "r469" |
///   "r47" | "r470" | "r471" | "r472" | "r473" | "r474" | "r475" | "r476" |
///   "r477" | "r478" | "r479" | "r48" | "r480" | "r481" | "r482" | "r483" |
///   "r484" | "r485" | "r486" | "r487" | "r488" | "r489" | "r49" | "r490" |
///   "r491" | "r492" | "r493" | "r494" | "r495" | "r496" | "r497" | "r498" |
///   "r499" | "r5" | "r50" | "r500" | "r501" | "r502" | "r503" | "r504" |
///   "r505" | "r506" | "r507" | "r508" | "r509" | "r51" | "r510" | "r511" |
///   "r512" | "r513" | "r514" | "r515" | "r516" | "r517" | "r518" | "r519" |
///   "r52" | "r520" | "r521" | "r522" | "r523" | "r524" | "r525" | "r526" |
///   "r527" | "r528" | "r529" | "r53" | "r530" | "r531" | "r532" | "r533" |
///   "r534" | "r535" | "r536" | "r537" | "r538" | "r539" | "r54" | "r540" |
///   "r541" | "r542" | "r543" | "r544" | "r545" | "r546" | "r547" | "r548" |
///   "r549" | "r55" | "r550" | "r551" | "r552" | "r553" | "r554" | "r555" |
///   "r556" | "r557" | "r558" | "r559" | "r56" | "r560" | "r561" | "r562" |
///   "r563" | "r564" | "r565" | "r566" | "r567" | "r568" | "r569" | "r57" |
///   "r570" | "r571" | "r572" | "r573" | "r574" | "r575" | "r576" | "r577" |
///   "r578" | "r579" | "r58" | "r580" | "r581" | "r582" | "r583" | "r584" |
///   "r585" | "r586" | "r587" | "r588" | "r589" | "r59" | "r590" | "r591" |
///   "r592" | "r593" | "r594" | "r595" | "r596" | "r597" | "r598" | "r599" |
///   "r6" | "r60" | "r600" | "r601" | "r602" | "r603" | "r604" | "r605" |
///   "r606" | "r607" | "r608" | "r609" | "r61" | "r610" | "r611" | "r612" |
///   "r613" | "r614" | "r615" | "r616" | "r617" | "r618" | "r619" | "r62" |
///   "r620" | "r621" | "r622" | "r623" | "r624" | "r625" | "r626" | "r627" |
///   "r628" | "r629" | "r63" | "r630" | "r631" | "r632" | "r633" | "r634" |
///   "r635" | "r636" | "r637" | "r638" | "r639" | "r64" | "r640" | "r641" |
///   "r642" | "r643" | "r644" | "r645" | "r646" | "r647" | "r648" | "r649" |
///   "r65" | "r650" | "r651" | "r652" | "r653" | "r654" | "r655" | "r656" |
///   "r657" | "r658" | "r659" | "r66" | "r660" | "r661" | "r662" | "r663" |
///   "r664" | "r665" | "r666" | "r667" | "r668" | "r669" | "r67" | "r670" |
///   "r671" | "r672" | "r673" | "r674" | "r675" | "r676" | "r677" | "r678" |
///   "r679" | "r68" | "r680" | "r681" | "r682" | "r683" | "r684" | "r685" |
///   "r686" | "r687" | "r688" | "r689" | "r69" | "r690" | "r691" | "r692" |
///   "r693" | "r694" | "r695" | "r696" | "r697" | "r698" | "r699" | "r7" |
///   "r70" | "r700" | "r701" | "r702" | "r703" | "r704" | "r705" | "r706" |
///   "r707" | "r708" | "r709" | "r71" | "r710" | "r711" | "r712" | "r713" |
///   "r714" | "r715" | "r716" | "r717" | "r718" | "r719" | "r72" | "r720" |
///   "r721" | "r722" | "r723" | "r724" | "r725" | "r726" | "r727" | "r728" |
///   "r729" | "r73" | "r730" | "r731" | "r732" | "r733" | "r734" | "r735" |
///   "r736" | "r737" | "r738" | "r739" | "r74" | "r740" | "r741" | "r742" |
///   "r743" | "r744" | "r745" | "r746" | "r747" | "r748" | "r749" | "r75" |
///   "r750" | "r751" | "r752" | "r753" | "r754" | "r755" | "r756" | "r757" |
///   "r758" | "r759" | "r76" | "r760" | "r761" | "r762" | "r763" | "r764" |
///   "r765" | "r766" | "r767" | "r768" | "r769" | "r77" | "r770" | "r771" |
///   "r772" | "r773" | "r774" | "r775" | "r776" | "r777" | "r778" | "r779" |
///   "r78" | "r780" | "r781" | "r782" | "r783" | "r784" | "r785" | "r786" |
///   "r787" | "r788" | "r789" | "r79" | "r790" | "r791" | "r792" | "r793" |
///   "r794" | "r795" | "r796" | "r797" | "r798" | "r799" | "r8" | "r80" | "r81"
///   | "r82" | "r83" | "r84" | "r85" | "r86" | "r87" | "r88" | "r89" | "r9" |
///   "r90" | "r91" | "r92" | "r93" | "r94" | "r95" | "r96" | "r97" | "r98" |
///   "r99"`; the lane produced no value: Failure(Budget(WorkBudgetExceeded)).
#[test]
#[ignore = "a function with 800 returning guards infers its return within the work budget"]
fn an_800_return_if_chain_answers_within_the_work_budget() {
    let matrix = Matrix::new(RETURN_CHAIN_800);
    let failures = matrix.returns(&[
        ("chain", "\"none\" | \"r0\" | \"r1\" | \"r10\" | \"r100\" | \"r101\" | \"r102\" | \"r103\" | \"r104\" | \"r105\" | \"r106\" | \"r107\" | \"r108\" | \"r109\" | \"r11\" | \"r110\" | \"r111\" | \"r112\" | \"r113\" | \"r114\" | \"r115\" | \"r116\" | \"r117\" | \"r118\" | \"r119\" | \"r12\" | \"r120\" | \"r121\" | \"r122\" | \"r123\" | \"r124\" | \"r125\" | \"r126\" | \"r127\" | \"r128\" | \"r129\" | \"r13\" | \"r130\" | \"r131\" | \"r132\" | \"r133\" | \"r134\" | \"r135\" | \"r136\" | \"r137\" | \"r138\" | \"r139\" | \"r14\" | \"r140\" | \"r141\" | \"r142\" | \"r143\" | \"r144\" | \"r145\" | \"r146\" | \"r147\" | \"r148\" | \"r149\" | \"r15\" | \"r150\" | \"r151\" | \"r152\" | \"r153\" | \"r154\" | \"r155\" | \"r156\" | \"r157\" | \"r158\" | \"r159\" | \"r16\" | \"r160\" | \"r161\" | \"r162\" | \"r163\" | \"r164\" | \"r165\" | \"r166\" | \"r167\" | \"r168\" | \"r169\" | \"r17\" | \"r170\" | \"r171\" | \"r172\" | \"r173\" | \"r174\" | \"r175\" | \"r176\" | \"r177\" | \"r178\" | \"r179\" | \"r18\" | \"r180\" | \"r181\" | \"r182\" | \"r183\" | \"r184\" | \"r185\" | \"r186\" | \"r187\" | \"r188\" | \"r189\" | \"r19\" | \"r190\" | \"r191\" | \"r192\" | \"r193\" | \"r194\" | \"r195\" | \"r196\" | \"r197\" | \"r198\" | \"r199\" | \"r2\" | \"r20\" | \"r200\" | \"r201\" | \"r202\" | \"r203\" | \"r204\" | \"r205\" | \"r206\" | \"r207\" | \"r208\" | \"r209\" | \"r21\" | \"r210\" | \"r211\" | \"r212\" | \"r213\" | \"r214\" | \"r215\" | \"r216\" | \"r217\" | \"r218\" | \"r219\" | \"r22\" | \"r220\" | \"r221\" | \"r222\" | \"r223\" | \"r224\" | \"r225\" | \"r226\" | \"r227\" | \"r228\" | \"r229\" | \"r23\" | \"r230\" | \"r231\" | \"r232\" | \"r233\" | \"r234\" | \"r235\" | \"r236\" | \"r237\" | \"r238\" | \"r239\" | \"r24\" | \"r240\" | \"r241\" | \"r242\" | \"r243\" | \"r244\" | \"r245\" | \"r246\" | \"r247\" | \"r248\" | \"r249\" | \"r25\" | \"r250\" | \"r251\" | \"r252\" | \"r253\" | \"r254\" | \"r255\" | \"r256\" | \"r257\" | \"r258\" | \"r259\" | \"r26\" | \"r260\" | \"r261\" | \"r262\" | \"r263\" | \"r264\" | \"r265\" | \"r266\" | \"r267\" | \"r268\" | \"r269\" | \"r27\" | \"r270\" | \"r271\" | \"r272\" | \"r273\" | \"r274\" | \"r275\" | \"r276\" | \"r277\" | \"r278\" | \"r279\" | \"r28\" | \"r280\" | \"r281\" | \"r282\" | \"r283\" | \"r284\" | \"r285\" | \"r286\" | \"r287\" | \"r288\" | \"r289\" | \"r29\" | \"r290\" | \"r291\" | \"r292\" | \"r293\" | \"r294\" | \"r295\" | \"r296\" | \"r297\" | \"r298\" | \"r299\" | \"r3\" | \"r30\" | \"r300\" | \"r301\" | \"r302\" | \"r303\" | \"r304\" | \"r305\" | \"r306\" | \"r307\" | \"r308\" | \"r309\" | \"r31\" | \"r310\" | \"r311\" | \"r312\" | \"r313\" | \"r314\" | \"r315\" | \"r316\" | \"r317\" | \"r318\" | \"r319\" | \"r32\" | \"r320\" | \"r321\" | \"r322\" | \"r323\" | \"r324\" | \"r325\" | \"r326\" | \"r327\" | \"r328\" | \"r329\" | \"r33\" | \"r330\" | \"r331\" | \"r332\" | \"r333\" | \"r334\" | \"r335\" | \"r336\" | \"r337\" | \"r338\" | \"r339\" | \"r34\" | \"r340\" | \"r341\" | \"r342\" | \"r343\" | \"r344\" | \"r345\" | \"r346\" | \"r347\" | \"r348\" | \"r349\" | \"r35\" | \"r350\" | \"r351\" | \"r352\" | \"r353\" | \"r354\" | \"r355\" | \"r356\" | \"r357\" | \"r358\" | \"r359\" | \"r36\" | \"r360\" | \"r361\" | \"r362\" | \"r363\" | \"r364\" | \"r365\" | \"r366\" | \"r367\" | \"r368\" | \"r369\" | \"r37\" | \"r370\" | \"r371\" | \"r372\" | \"r373\" | \"r374\" | \"r375\" | \"r376\" | \"r377\" | \"r378\" | \"r379\" | \"r38\" | \"r380\" | \"r381\" | \"r382\" | \"r383\" | \"r384\" | \"r385\" | \"r386\" | \"r387\" | \"r388\" | \"r389\" | \"r39\" | \"r390\" | \"r391\" | \"r392\" | \"r393\" | \"r394\" | \"r395\" | \"r396\" | \"r397\" | \"r398\" | \"r399\" | \"r4\" | \"r40\" | \"r400\" | \"r401\" | \"r402\" | \"r403\" | \"r404\" | \"r405\" | \"r406\" | \"r407\" | \"r408\" | \"r409\" | \"r41\" | \"r410\" | \"r411\" | \"r412\" | \"r413\" | \"r414\" | \"r415\" | \"r416\" | \"r417\" | \"r418\" | \"r419\" | \"r42\" | \"r420\" | \"r421\" | \"r422\" | \"r423\" | \"r424\" | \"r425\" | \"r426\" | \"r427\" | \"r428\" | \"r429\" | \"r43\" | \"r430\" | \"r431\" | \"r432\" | \"r433\" | \"r434\" | \"r435\" | \"r436\" | \"r437\" | \"r438\" | \"r439\" | \"r44\" | \"r440\" | \"r441\" | \"r442\" | \"r443\" | \"r444\" | \"r445\" | \"r446\" | \"r447\" | \"r448\" | \"r449\" | \"r45\" | \"r450\" | \"r451\" | \"r452\" | \"r453\" | \"r454\" | \"r455\" | \"r456\" | \"r457\" | \"r458\" | \"r459\" | \"r46\" | \"r460\" | \"r461\" | \"r462\" | \"r463\" | \"r464\" | \"r465\" | \"r466\" | \"r467\" | \"r468\" | \"r469\" | \"r47\" | \"r470\" | \"r471\" | \"r472\" | \"r473\" | \"r474\" | \"r475\" | \"r476\" | \"r477\" | \"r478\" | \"r479\" | \"r48\" | \"r480\" | \"r481\" | \"r482\" | \"r483\" | \"r484\" | \"r485\" | \"r486\" | \"r487\" | \"r488\" | \"r489\" | \"r49\" | \"r490\" | \"r491\" | \"r492\" | \"r493\" | \"r494\" | \"r495\" | \"r496\" | \"r497\" | \"r498\" | \"r499\" | \"r5\" | \"r50\" | \"r500\" | \"r501\" | \"r502\" | \"r503\" | \"r504\" | \"r505\" | \"r506\" | \"r507\" | \"r508\" | \"r509\" | \"r51\" | \"r510\" | \"r511\" | \"r512\" | \"r513\" | \"r514\" | \"r515\" | \"r516\" | \"r517\" | \"r518\" | \"r519\" | \"r52\" | \"r520\" | \"r521\" | \"r522\" | \"r523\" | \"r524\" | \"r525\" | \"r526\" | \"r527\" | \"r528\" | \"r529\" | \"r53\" | \"r530\" | \"r531\" | \"r532\" | \"r533\" | \"r534\" | \"r535\" | \"r536\" | \"r537\" | \"r538\" | \"r539\" | \"r54\" | \"r540\" | \"r541\" | \"r542\" | \"r543\" | \"r544\" | \"r545\" | \"r546\" | \"r547\" | \"r548\" | \"r549\" | \"r55\" | \"r550\" | \"r551\" | \"r552\" | \"r553\" | \"r554\" | \"r555\" | \"r556\" | \"r557\" | \"r558\" | \"r559\" | \"r56\" | \"r560\" | \"r561\" | \"r562\" | \"r563\" | \"r564\" | \"r565\" | \"r566\" | \"r567\" | \"r568\" | \"r569\" | \"r57\" | \"r570\" | \"r571\" | \"r572\" | \"r573\" | \"r574\" | \"r575\" | \"r576\" | \"r577\" | \"r578\" | \"r579\" | \"r58\" | \"r580\" | \"r581\" | \"r582\" | \"r583\" | \"r584\" | \"r585\" | \"r586\" | \"r587\" | \"r588\" | \"r589\" | \"r59\" | \"r590\" | \"r591\" | \"r592\" | \"r593\" | \"r594\" | \"r595\" | \"r596\" | \"r597\" | \"r598\" | \"r599\" | \"r6\" | \"r60\" | \"r600\" | \"r601\" | \"r602\" | \"r603\" | \"r604\" | \"r605\" | \"r606\" | \"r607\" | \"r608\" | \"r609\" | \"r61\" | \"r610\" | \"r611\" | \"r612\" | \"r613\" | \"r614\" | \"r615\" | \"r616\" | \"r617\" | \"r618\" | \"r619\" | \"r62\" | \"r620\" | \"r621\" | \"r622\" | \"r623\" | \"r624\" | \"r625\" | \"r626\" | \"r627\" | \"r628\" | \"r629\" | \"r63\" | \"r630\" | \"r631\" | \"r632\" | \"r633\" | \"r634\" | \"r635\" | \"r636\" | \"r637\" | \"r638\" | \"r639\" | \"r64\" | \"r640\" | \"r641\" | \"r642\" | \"r643\" | \"r644\" | \"r645\" | \"r646\" | \"r647\" | \"r648\" | \"r649\" | \"r65\" | \"r650\" | \"r651\" | \"r652\" | \"r653\" | \"r654\" | \"r655\" | \"r656\" | \"r657\" | \"r658\" | \"r659\" | \"r66\" | \"r660\" | \"r661\" | \"r662\" | \"r663\" | \"r664\" | \"r665\" | \"r666\" | \"r667\" | \"r668\" | \"r669\" | \"r67\" | \"r670\" | \"r671\" | \"r672\" | \"r673\" | \"r674\" | \"r675\" | \"r676\" | \"r677\" | \"r678\" | \"r679\" | \"r68\" | \"r680\" | \"r681\" | \"r682\" | \"r683\" | \"r684\" | \"r685\" | \"r686\" | \"r687\" | \"r688\" | \"r689\" | \"r69\" | \"r690\" | \"r691\" | \"r692\" | \"r693\" | \"r694\" | \"r695\" | \"r696\" | \"r697\" | \"r698\" | \"r699\" | \"r7\" | \"r70\" | \"r700\" | \"r701\" | \"r702\" | \"r703\" | \"r704\" | \"r705\" | \"r706\" | \"r707\" | \"r708\" | \"r709\" | \"r71\" | \"r710\" | \"r711\" | \"r712\" | \"r713\" | \"r714\" | \"r715\" | \"r716\" | \"r717\" | \"r718\" | \"r719\" | \"r72\" | \"r720\" | \"r721\" | \"r722\" | \"r723\" | \"r724\" | \"r725\" | \"r726\" | \"r727\" | \"r728\" | \"r729\" | \"r73\" | \"r730\" | \"r731\" | \"r732\" | \"r733\" | \"r734\" | \"r735\" | \"r736\" | \"r737\" | \"r738\" | \"r739\" | \"r74\" | \"r740\" | \"r741\" | \"r742\" | \"r743\" | \"r744\" | \"r745\" | \"r746\" | \"r747\" | \"r748\" | \"r749\" | \"r75\" | \"r750\" | \"r751\" | \"r752\" | \"r753\" | \"r754\" | \"r755\" | \"r756\" | \"r757\" | \"r758\" | \"r759\" | \"r76\" | \"r760\" | \"r761\" | \"r762\" | \"r763\" | \"r764\" | \"r765\" | \"r766\" | \"r767\" | \"r768\" | \"r769\" | \"r77\" | \"r770\" | \"r771\" | \"r772\" | \"r773\" | \"r774\" | \"r775\" | \"r776\" | \"r777\" | \"r778\" | \"r779\" | \"r78\" | \"r780\" | \"r781\" | \"r782\" | \"r783\" | \"r784\" | \"r785\" | \"r786\" | \"r787\" | \"r788\" | \"r789\" | \"r79\" | \"r790\" | \"r791\" | \"r792\" | \"r793\" | \"r794\" | \"r795\" | \"r796\" | \"r797\" | \"r798\" | \"r799\" | \"r8\" | \"r80\" | \"r81\" | \"r82\" | \"r83\" | \"r84\" | \"r85\" | \"r86\" | \"r87\" | \"r88\" | \"r89\" | \"r9\" | \"r90\" | \"r91\" | \"r92\" | \"r93\" | \"r94\" | \"r95\" | \"r96\" | \"r97\" | \"r98\" | \"r99\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A conditional alias with 160 nested `T extends i ? … :` branches.
const CONDITIONAL_CHAIN_160: &str = r##"type Pick1<T> = T extends 0 ? "c0" : T extends 1 ? "c1" : T extends 2 ? "c2" : T extends 3 ? "c3" : T extends 4 ? "c4" : T extends 5 ? "c5" : T extends 6 ? "c6" : T extends 7 ? "c7" : T extends 8 ? "c8" : T extends 9 ? "c9" : T extends 10 ? "c10" : T extends 11 ? "c11" : T extends 12 ? "c12" : T extends 13 ? "c13" : T extends 14 ? "c14" : T extends 15 ? "c15" : T extends 16 ? "c16" : T extends 17 ? "c17" : T extends 18 ? "c18" : T extends 19 ? "c19" : T extends 20 ? "c20" : T extends 21 ? "c21" : T extends 22 ? "c22" : T extends 23 ? "c23" : T extends 24 ? "c24" : T extends 25 ? "c25" : T extends 26 ? "c26" : T extends 27 ? "c27" : T extends 28 ? "c28" : T extends 29 ? "c29" : T extends 30 ? "c30" : T extends 31 ? "c31" : T extends 32 ? "c32" : T extends 33 ? "c33" : T extends 34 ? "c34" : T extends 35 ? "c35" : T extends 36 ? "c36" : T extends 37 ? "c37" : T extends 38 ? "c38" : T extends 39 ? "c39" : T extends 40 ? "c40" : T extends 41 ? "c41" : T extends 42 ? "c42" : T extends 43 ? "c43" : T extends 44 ? "c44" : T extends 45 ? "c45" : T extends 46 ? "c46" : T extends 47 ? "c47" : T extends 48 ? "c48" : T extends 49 ? "c49" : T extends 50 ? "c50" : T extends 51 ? "c51" : T extends 52 ? "c52" : T extends 53 ? "c53" : T extends 54 ? "c54" : T extends 55 ? "c55" : T extends 56 ? "c56" : T extends 57 ? "c57" : T extends 58 ? "c58" : T extends 59 ? "c59" : T extends 60 ? "c60" : T extends 61 ? "c61" : T extends 62 ? "c62" : T extends 63 ? "c63" : T extends 64 ? "c64" : T extends 65 ? "c65" : T extends 66 ? "c66" : T extends 67 ? "c67" : T extends 68 ? "c68" : T extends 69 ? "c69" : T extends 70 ? "c70" : T extends 71 ? "c71" : T extends 72 ? "c72" : T extends 73 ? "c73" : T extends 74 ? "c74" : T extends 75 ? "c75" : T extends 76 ? "c76" : T extends 77 ? "c77" : T extends 78 ? "c78" : T extends 79 ? "c79" : T extends 80 ? "c80" : T extends 81 ? "c81" : T extends 82 ? "c82" : T extends 83 ? "c83" : T extends 84 ? "c84" : T extends 85 ? "c85" : T extends 86 ? "c86" : T extends 87 ? "c87" : T extends 88 ? "c88" : T extends 89 ? "c89" : T extends 90 ? "c90" : T extends 91 ? "c91" : T extends 92 ? "c92" : T extends 93 ? "c93" : T extends 94 ? "c94" : T extends 95 ? "c95" : T extends 96 ? "c96" : T extends 97 ? "c97" : T extends 98 ? "c98" : T extends 99 ? "c99" : T extends 100 ? "c100" : T extends 101 ? "c101" : T extends 102 ? "c102" : T extends 103 ? "c103" : T extends 104 ? "c104" : T extends 105 ? "c105" : T extends 106 ? "c106" : T extends 107 ? "c107" : T extends 108 ? "c108" : T extends 109 ? "c109" : T extends 110 ? "c110" : T extends 111 ? "c111" : T extends 112 ? "c112" : T extends 113 ? "c113" : T extends 114 ? "c114" : T extends 115 ? "c115" : T extends 116 ? "c116" : T extends 117 ? "c117" : T extends 118 ? "c118" : T extends 119 ? "c119" : T extends 120 ? "c120" : T extends 121 ? "c121" : T extends 122 ? "c122" : T extends 123 ? "c123" : T extends 124 ? "c124" : T extends 125 ? "c125" : T extends 126 ? "c126" : T extends 127 ? "c127" : T extends 128 ? "c128" : T extends 129 ? "c129" : T extends 130 ? "c130" : T extends 131 ? "c131" : T extends 132 ? "c132" : T extends 133 ? "c133" : T extends 134 ? "c134" : T extends 135 ? "c135" : T extends 136 ? "c136" : T extends 137 ? "c137" : T extends 138 ? "c138" : T extends 139 ? "c139" : T extends 140 ? "c140" : T extends 141 ? "c141" : T extends 142 ? "c142" : T extends 143 ? "c143" : T extends 144 ? "c144" : T extends 145 ? "c145" : T extends 146 ? "c146" : T extends 147 ? "c147" : T extends 148 ? "c148" : T extends 149 ? "c149" : T extends 150 ? "c150" : T extends 151 ? "c151" : T extends 152 ? "c152" : T extends 153 ? "c153" : T extends 154 ? "c154" : T extends 155 ? "c155" : T extends 156 ? "c156" : T extends 157 ? "c157" : T extends 158 ? "c158" : T extends 159 ? "c159" : "none";
"##;

/// `Pick1<159>` over 160 nested conditional branches is `"c159"`. The lane
/// overflows its stack and aborts the process (80 branches answer in 0.2 s; 240
/// overflow too). Run it alone.
///
/// What the lane gives:
/// - `Pick1<159>`: the checker answers `"c159"`; the lane the process aborts:
///   `thread '<unknown>' has overflowed its stack`.
#[test]
#[ignore = "run alone: resolving a 160-deep conditional chain overflows the stack"]
fn a_160_deep_conditional_chain_resolves_on_the_default_stack() {
    let matrix = Matrix::new(CONDITIONAL_CHAIN_160);
    let failures = matrix.types(&[("Pick1<159>", "\"c159\"")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A loop rotating 20 declared-union locals.
const LOOP_LOCALS_20: &str = r##"declare function cond(): boolean;
export function lv() {
  let v0: string | number = 0;
  let v1: string | number = 0;
  let v2: string | number = 0;
  let v3: string | number = 0;
  let v4: string | number = 0;
  let v5: string | number = 0;
  let v6: string | number = 0;
  let v7: string | number = 0;
  let v8: string | number = 0;
  let v9: string | number = 0;
  let v10: string | number = 0;
  let v11: string | number = 0;
  let v12: string | number = 0;
  let v13: string | number = 0;
  let v14: string | number = 0;
  let v15: string | number = 0;
  let v16: string | number = 0;
  let v17: string | number = 0;
  let v18: string | number = 0;
  let v19: string | number = 0;
  while (cond()) {
    v0 = v1;
    v1 = v2;
    v2 = v3;
    v3 = v4;
    v4 = v5;
    v5 = v6;
    v6 = v7;
    v7 = v8;
    v8 = v9;
    v9 = v10;
    v10 = v11;
    v11 = v12;
    v12 = v13;
    v13 = v14;
    v14 = v15;
    v15 = v16;
    v16 = v17;
    v17 = v18;
    v18 = v19;
    v19 = v0;
    v19 = "s";
  }
  return v0;
}
"##;

/// `while (cond()) { v0 = v1; …; v19 = v0; v19 = "s"; } return v0;` over twenty
/// `let v<i>: string | number = 0` is `string | number`. The lane's time grows
/// exponentially with the locals: 5 locals 0.2 s, 10 23.5 s for the four
/// settings; at 20 every setting passes the row deadline.
///
/// What the lane gives:
/// - `lv`: the checker answers `string | number`; the lane took longer than
///   60s.
#[test]
#[ignore = "a loop whose locals feed each other reaches its fixed point in time the checker takes"]
fn a_loop_rotating_twenty_locals_reaches_its_fixed_point() {
    let matrix = Matrix::new(LOOP_LOCALS_20);
    let failures = matrix.returns(&[("lv", "string | number")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A loop rotating 5 declared-union locals.
const LOOP_LOCALS_5: &str = r##"declare function cond(): boolean;
export function lv() {
  let v0: string | number = 0;
  let v1: string | number = 0;
  let v2: string | number = 0;
  let v3: string | number = 0;
  let v4: string | number = 0;
  while (cond()) {
    v0 = v1;
    v1 = v2;
    v2 = v3;
    v3 = v4;
    v4 = v0;
    v4 = "s";
  }
  return v0;
}
"##;

/// A loop rotating five `string | number` locals, one of them written `"s"`,
/// leaves the first one `string | number`.
#[test]
fn a_loop_rotating_five_locals_reaches_its_fixed_point() {
    let matrix = Matrix::new(LOOP_LOCALS_5);
    let failures = matrix.returns(&[("lv", "string | number")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A 320-operand `&&` chain over five parameters.
const AND_CHAIN_320: &str = r##"export function lc(a0: string | undefined, a1: number | null, a2: boolean, a3: "x" | "", a4: 0 | 1) { return a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4 && a0 && a1 && a2 && a3 && a4; }
"##;

/// A 320-operand `&&` chain cycling five parameters is `"" | 0 | 1 | false |
/// null | undefined` (`0 | 1` without `strictNullChecks`). The lane's time
/// grows quadratically with the operands: 10 0.1 s, 20 0.4 s, 40 1.9 s, 80 8.4
/// s for the four settings; at 320 every setting passes the row deadline.
///
/// What the lane gives:
/// - `lc`: the checker answers `"" | 0 | 1 | false | null | undefined`
///   (strict), `0 | 1` (strictNullChecks off), `"" | 0 | 1 | false | null |
///   undefined` (noImplicitAny off), `0 | 1` (both off); the lane took longer
///   than 60s.
#[test]
#[ignore = "a long && chain types in time the checker takes"]
fn a_320_operand_and_chain_answers() {
    let matrix = Matrix::new(AND_CHAIN_320);
    let failures = matrix.nullness(&[(
        Read::Return("lc"),
        "\"\" | 0 | 1 | false | null | undefined",
        "0 | 1",
    )]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// 65 overloads of one declared function.
const OVERLOADS_65: &str = r##"declare function ov(a: 0): "r0";
declare function ov(a: 1): "r1";
declare function ov(a: 2): "r2";
declare function ov(a: 3): "r3";
declare function ov(a: 4): "r4";
declare function ov(a: 5): "r5";
declare function ov(a: 6): "r6";
declare function ov(a: 7): "r7";
declare function ov(a: 8): "r8";
declare function ov(a: 9): "r9";
declare function ov(a: 10): "r10";
declare function ov(a: 11): "r11";
declare function ov(a: 12): "r12";
declare function ov(a: 13): "r13";
declare function ov(a: 14): "r14";
declare function ov(a: 15): "r15";
declare function ov(a: 16): "r16";
declare function ov(a: 17): "r17";
declare function ov(a: 18): "r18";
declare function ov(a: 19): "r19";
declare function ov(a: 20): "r20";
declare function ov(a: 21): "r21";
declare function ov(a: 22): "r22";
declare function ov(a: 23): "r23";
declare function ov(a: 24): "r24";
declare function ov(a: 25): "r25";
declare function ov(a: 26): "r26";
declare function ov(a: 27): "r27";
declare function ov(a: 28): "r28";
declare function ov(a: 29): "r29";
declare function ov(a: 30): "r30";
declare function ov(a: 31): "r31";
declare function ov(a: 32): "r32";
declare function ov(a: 33): "r33";
declare function ov(a: 34): "r34";
declare function ov(a: 35): "r35";
declare function ov(a: 36): "r36";
declare function ov(a: 37): "r37";
declare function ov(a: 38): "r38";
declare function ov(a: 39): "r39";
declare function ov(a: 40): "r40";
declare function ov(a: 41): "r41";
declare function ov(a: 42): "r42";
declare function ov(a: 43): "r43";
declare function ov(a: 44): "r44";
declare function ov(a: 45): "r45";
declare function ov(a: 46): "r46";
declare function ov(a: 47): "r47";
declare function ov(a: 48): "r48";
declare function ov(a: 49): "r49";
declare function ov(a: 50): "r50";
declare function ov(a: 51): "r51";
declare function ov(a: 52): "r52";
declare function ov(a: 53): "r53";
declare function ov(a: 54): "r54";
declare function ov(a: 55): "r55";
declare function ov(a: 56): "r56";
declare function ov(a: 57): "r57";
declare function ov(a: 58): "r58";
declare function ov(a: 59): "r59";
declare function ov(a: 60): "r60";
declare function ov(a: 61): "r61";
declare function ov(a: 62): "r62";
declare function ov(a: 63): "r63";
declare function ov(a: 64): "r64";
export function useLast() { return ov(64); }
export function useFirst() { return ov(0); }
"##;

/// A call matching the first of 65 overloads resolves it.
#[test]
fn the_first_of_65_overloads_resolves() {
    let matrix = Matrix::new(OVERLOADS_65);
    let failures = matrix.returns(&[("useFirst", "\"r0\"")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `ov(64)` resolves the 65th overload `(a: 64): "r64"`. The lane gives up with
/// `UnrepresentableCallee` whenever the match lies past the 64th overload (64
/// overloads resolve their last).
///
/// What the lane gives:
/// - `useLast`: the checker answers `"r64"`; the lane measured `<opaque
///   UnmodeledPosition>` degraded by UnrepresentableCallee.
#[test]
#[ignore = "a call resolves an overload past the sixty-fourth"]
fn the_sixty_fifth_overload_resolves() {
    let matrix = Matrix::new(OVERLOADS_65);
    let failures = matrix.returns(&[("useLast", "\"r64\"")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A class and an interface whose `next()` returns the same type.
const METHOD_CHAINS: &str = r##"class Ch { v = 1 as const; next(): Ch { return this; } }
export function mc0(c: Ch) { return c.v; }
export function mc1(c: Ch) { return c.next().v; }
export function mc2(c: Ch) { return c.next().next().v; }
interface ICh { v: 1; next(): ICh }
export function mi2(c: ICh) { return c.next().next().v; }
"##;

/// A member read on the receiver, and on the result of one `next()` call, reads
/// the member.
#[test]
fn a_single_method_call_reads_through() {
    let matrix = Matrix::new(METHOD_CHAINS);
    let failures = matrix.returns(&[("mc0", "1"), ("mc1", "1")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `c.next().next().v` is `1` for a class and for an interface receiver; the
/// lane answers one call (`c.next().v`) but not two.
///
/// What the lane gives:
/// - `mc2`: the checker answers `1`; the lane measured `<opaque
///   UnmodeledPosition>` degraded by UnmodeledPosition.
/// - `mi2`: the checker answers `1`; the lane measured `<opaque
///   UnmodeledPosition>` degraded by UnmodeledPosition.
#[test]
#[ignore = "a member read after two chained method calls resolves"]
fn a_chain_of_two_method_calls_reads_through() {
    let matrix = Matrix::new(METHOD_CHAINS);
    let failures = matrix.returns(&[("mc2", "1"), ("mi2", "1")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A small object type.
const INFER_DISTRIBUTION: &str = r##"type O = { a: 1; p0: 2 };
"##;

/// `O extends infer K ? K : 2` is `O`.
#[test]
fn an_infer_bound_non_union_passes_through() {
    let matrix = Matrix::new(INFER_DISTRIBUTION);
    let failures = matrix.types(&[("O extends infer K ? K : 2", "O")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `K extends "p0" ? 1 : 0` with `K` inferred as a union (`keyof O extends
/// infer K`, `"a" | "b" extends infer K`, `["a" | "b"] extends [infer K]`)
/// distributes over `K`: `0 | 1`. Wrong-but-clean: the lane answers `0`.
///
/// What the lane gives:
/// - `keyof O extends infer K ? K extends "p0" ? 1 : 0 : 2`: the checker
///   answers `0 | 1`; the lane measured `0`.
/// - `"a" | "b" extends infer K ? K extends "a" ? 1 : 0 : 2`: the checker
///   answers `0 | 1`; the lane measured `0`.
/// - `["a" | "b"] extends [infer K] ? K extends "a" ? 1 : 0 : 2`: the checker
///   answers `0 | 1`; the lane measured `0`.
#[test]
#[ignore = "a conditional whose check type is an infer-bound union distributes over it"]
fn wrong_clean_a_conditional_over_an_infer_bound_union_distributes() {
    let matrix = Matrix::new(INFER_DISTRIBUTION);
    let failures = matrix.types(&[
        (
            "keyof O extends infer K ? K extends \"p0\" ? 1 : 0 : 2",
            "0 | 1",
        ),
        (
            "\"a\" | \"b\" extends infer K ? K extends \"a\" ? 1 : 0 : 2",
            "0 | 1",
        ),
        (
            "[\"a\" | \"b\"] extends [infer K] ? K extends \"a\" ? 1 : 0 : 2",
            "0 | 1",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
