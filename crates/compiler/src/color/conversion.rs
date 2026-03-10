// Color space conversion matrices require full-precision constants from specifications.
#![allow(clippy::excessive_precision)]

//! Color space conversion functions.
//!
//! All conversions go through a common linear space (linear sRGB or XYZ-D65)
//! as an interchange format. The general pipeline is:
//!
//!   source space -> (to linear if needed) -> linear sRGB -> XYZ-D65 -> linear target -> target space
//!
//! For spaces that share a linear base (e.g. sRGB and Display P3 both go through
//! XYZ-D65), we can skip intermediate steps.
//!
//! All matrix constants are from the CSS Color Level 4 specification and match
//! dart-sass's implementation exactly.

use std::f64::consts::PI;

use super::space::ColorSpace;

// ---- 3x3 Matrix operations ----

/// Row-major 3x3 matrix
type Mat3 = [f64; 9];

/// Multiply a 3x3 matrix by a 3-element column vector
fn mat3_mul(m: &Mat3, v: [f64; 3]) -> [f64; 3] {
    [
        m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
        m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
        m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
    ]
}

// ---- Linear <-> XYZ-D65 transformation matrices ----
// Source: CSS Color Level 4 spec, matching dart-sass

/// sRGB linear -> XYZ-D65
const SRGB_LINEAR_TO_XYZ_D65: Mat3 = [
    0.41239079926595950, 0.35758433938387796, 0.18048078840183430,
    0.21263900587151036, 0.71516867876775590, 0.07219231536073371,
    0.01933081871559185, 0.11919477979462598, 0.95053215224966060,
];

/// XYZ-D65 -> sRGB linear
const XYZ_D65_TO_SRGB_LINEAR: Mat3 = [
     3.24096994190452130, -1.53738317757009350, -0.49861076029300330,
    -0.96924363628087980,  1.87596750150772060,  0.04155505740717561,
     0.05563007969699360, -0.20397695888897657,  1.05697151424287860,
];

/// Display P3 linear -> XYZ-D65
const DISPLAY_P3_LINEAR_TO_XYZ_D65: Mat3 = [
    0.48657094864821626, 0.26566769316909294, 0.19821728523436250,
    0.22897456406974884, 0.69173852183650620, 0.07928691409374500,
    0.00000000000000000, 0.04511338185890257, 1.04394436890097570,
];

/// XYZ-D65 -> Display P3 linear
const XYZ_D65_TO_DISPLAY_P3_LINEAR: Mat3 = [
     2.49349691194142450, -0.93138361791912360, -0.40271078445071684,
    -0.82948896956157490,  1.76266406031834680,  0.02362468584194359,
     0.03584583024378433, -0.07617238926804170,  0.95688452400768730,
];

/// A98 RGB linear -> XYZ-D65
const A98_RGB_LINEAR_TO_XYZ_D65: Mat3 = [
    0.57666904291013080, 0.18555823790654627, 0.18822864623499472,
    0.29734497525053616, 0.62736356625546600, 0.07529145849399789,
    0.02703136138641237, 0.07068885253582714, 0.99133753683763890,
];

/// XYZ-D65 -> A98 RGB linear
const XYZ_D65_TO_A98_RGB_LINEAR: Mat3 = [
     2.04158790381074600, -0.56500697427885960, -0.34473135077832950,
    -0.96924363628087980,  1.87596750150772060,  0.04155505740717561,
     0.01344428063203102, -0.11836239223101823,  1.01517499439120540,
];

/// ProPhoto RGB linear -> XYZ-D50
const PROPHOTO_RGB_LINEAR_TO_XYZ_D50: Mat3 = [
    0.79776664490064230, 0.13518129740053308, 0.03134773412839220,
    0.28807482881940130, 0.71183523424187300, 0.00008993693872564,
    0.00000000000000000, 0.00000000000000000, 0.82510460251046020,
];

/// XYZ-D50 -> ProPhoto RGB linear
const XYZ_D50_TO_PROPHOTO_RGB_LINEAR: Mat3 = [
     1.34578688164715830, -0.25557208737979464, -0.05110186497554526,
    -0.54463070512490190,  1.50824774284514680,  0.02052744743642139,
     0.00000000000000000,  0.00000000000000000,  1.21196754563894520,
];

/// Rec2020 linear -> XYZ-D65
const REC2020_LINEAR_TO_XYZ_D65: Mat3 = [
    0.63695804830129130, 0.14461690358620838, 0.16888097516417205,
    0.26270021201126703, 0.67799807151887100, 0.05930171646986194,
    0.00000000000000000, 0.02807269304908750, 1.06098505771079090,
];

/// XYZ-D65 -> Rec2020 linear
const XYZ_D65_TO_REC2020_LINEAR: Mat3 = [
     1.71665118797126760, -0.35567078377639240, -0.25336628137365980,
    -0.66668435183248900,  1.61648123663493900,  0.01576854581391113,
     0.01763985744531091, -0.04277061325780865,  0.94210312123547400,
];

/// XYZ-D50 -> XYZ-D65 (Bradford chromatic adaptation)
const XYZ_D50_TO_XYZ_D65: Mat3 = [
     0.95547342148807520, -0.02309845494876452, 0.06325924320057065,
    -0.02836970933386358,  1.00999539808130410,  0.02104144119191730,
     0.01231401486448199, -0.02050764929889898,  1.33036592624212400,
];

/// XYZ-D65 -> XYZ-D50 (Bradford chromatic adaptation)
const XYZ_D65_TO_XYZ_D50: Mat3 = [
     1.04792979254499660,  0.02294687060160952, -0.05019226628920519,
     0.02962780877005567,  0.99043442675388000, -0.01707379906341879,
    -0.00924304064620452,  0.01505519149029816,  0.75187428142813700,
];

// ---- Direct linear-to-linear conversion matrices ----
// Pre-composed matrices that convert directly between RGB-family linear spaces
// without going through XYZ-D65. This avoids the FP rounding error introduced
// by chaining two matrix multiplications. All values match dart-sass exactly.

// sRGB-linear ↔ Display P3 linear
const SRGB_LINEAR_TO_DISPLAY_P3_LINEAR: Mat3 = [
    0.82246196871436230, 0.17753803128563775, 0.00000000000000000,
    0.03319419885096161, 0.96680580114903840, 0.00000000000000000,
    0.01708263072112003, 0.07239744066396346, 0.91051992861491650,
];
const DISPLAY_P3_LINEAR_TO_SRGB_LINEAR: Mat3 = [
    1.22494017628055980, -0.22494017628055996, 0.00000000000000000,
   -0.04205695470968816,  1.04205695470968800, 0.00000000000000000,
   -0.01963755459033443, -0.07863604555063188, 1.09827360014096630,
];

// sRGB-linear ↔ A98 RGB linear
const SRGB_LINEAR_TO_A98_RGB_LINEAR: Mat3 = [
    0.71512560685562470, 0.28487439314437535, 0.00000000000000000,
    0.00000000000000000, 1.00000000000000000, 0.00000000000000000,
    0.00000000000000000, 0.04116194845011846, 0.95883805154988160,
];
const A98_RGB_LINEAR_TO_SRGB_LINEAR: Mat3 = [
    1.39835574396077830, -0.39835574396077830, 0.00000000000000000,
    0.00000000000000000,  1.00000000000000000, 0.00000000000000000,
    0.00000000000000000, -0.04292898929447326, 1.04292898929447330,
];

// sRGB-linear ↔ Rec2020 linear
const SRGB_LINEAR_TO_REC2020_LINEAR: Mat3 = [
    0.62740389593469900, 0.32928303837788370, 0.04331306568741722,
    0.06909728935823208, 0.91954039507545870, 0.01136231556630917,
    0.01639143887515027, 0.08801330787722575, 0.89559525324762400,
];
const REC2020_LINEAR_TO_SRGB_LINEAR: Mat3 = [
     1.66049100210843450, -0.58764113878854950, -0.07284986331988487,
    -0.12455047452159074,  1.13289989712596030, -0.00834942260436947,
    -0.01815076335490530, -0.10057889800800737,  1.11872966136291270,
];

// sRGB-linear ↔ ProPhoto RGB linear
const SRGB_LINEAR_TO_PROPHOTO_RGB_LINEAR: Mat3 = [
    0.52927697762261160, 0.33015450197849283, 0.14056852039889556,
    0.09836585954044917, 0.87347071290696180, 0.02816342755258900,
    0.01687534092138684, 0.11765941425612084, 0.86546524482249230,
];
const PROPHOTO_RGB_LINEAR_TO_SRGB_LINEAR: Mat3 = [
     2.03438084951699600, -0.72763578993413420, -0.30674505958286180,
    -0.22882573163305037,  1.23174254119010480, -0.00291680955705449,
    -0.00855882878391742, -0.15326670213803720,  1.16182553092195470,
];

// Display P3 linear ↔ A98 RGB linear
const DISPLAY_P3_LINEAR_TO_A98_RGB_LINEAR: Mat3 = [
    0.86400513747404840, 0.13599486252595164, 0.00000000000000000,
   -0.04205695470968816, 1.04205695470968800, 0.00000000000000000,
   -0.02056038078232985, -0.03250613804550798, 1.05306651882783790,
];
const A98_RGB_LINEAR_TO_DISPLAY_P3_LINEAR: Mat3 = [
    1.15009441814101840, -0.15009441814101834, 0.00000000000000000,
    0.04641729862941844,  0.95358270137058150, 0.00000000000000000,
    0.02388759479083904,  0.02650477632633013, 0.94960762888283080,
];

// Display P3 linear ↔ Rec2020 linear
const DISPLAY_P3_LINEAR_TO_REC2020_LINEAR: Mat3 = [
    0.75383303436172180, 0.19859736905261630, 0.04756959658566187,
    0.04574384896535833, 0.94177721981169350, 0.01247893122294812,
   -0.00121034035451832, 0.01760171730108989, 0.98360862305342840,
];
const REC2020_LINEAR_TO_DISPLAY_P3_LINEAR: Mat3 = [
     1.34357825258433200, -0.28217967052613570, -0.06139858205819628,
    -0.06529745278911953,  1.07578791584857460, -0.01049046305945495,
     0.00282178726170095, -0.01959849452449406,  1.01677670726279310,
];

// Display P3 linear ↔ ProPhoto RGB linear
const DISPLAY_P3_LINEAR_TO_PROPHOTO_RGB_LINEAR: Mat3 = [
    0.63168691934035890, 0.21393038569465722, 0.15438269496498390,
    0.08320371426648458, 0.88586513676302430, 0.03093114897049121,
   -0.00127273456473881, 0.05075510433665735, 0.95051763022808140,
];
const PROPHOTO_RGB_LINEAR_TO_DISPLAY_P3_LINEAR: Mat3 = [
     1.63257560870691790, -0.37977161848259840, -0.25280399022431950,
    -0.15370040233755072,  1.16670254724250140, -0.01300214490495082,
     0.01039319529676572, -0.06280731264959440,  1.05241411735282870,
];

// A98 RGB linear ↔ Rec2020 linear
const A98_RGB_LINEAR_TO_REC2020_LINEAR: Mat3 = [
    0.87733384166365680, 0.07749370651571998, 0.04517245182062317,
    0.09662259146620378, 0.89152732024418050, 0.01185008828961569,
    0.02292106270284839, 0.04303668501067932, 0.93404225228647230,
];
const REC2020_LINEAR_TO_A98_RGB_LINEAR: Mat3 = [
     1.15197839471591630, -0.09750305530240860, -0.05447533941350766,
    -0.12455047452159074,  1.13289989712596030, -0.00834942260436947,
    -0.02253038278105590, -0.04980650742838876,  1.07233689020944460,
];

// A98 RGB linear ↔ ProPhoto RGB linear
const A98_RGB_LINEAR_TO_PROPHOTO_RGB_LINEAR: Mat3 = [
    0.74011750180477920, 0.11327951328898105, 0.14660298490623970,
    0.13755046469802620, 0.83307708026948400, 0.02937245503248977,
    0.02359772990871766, 0.07378347703906656, 0.90261879305221580,
];
const PROPHOTO_RGB_LINEAR_TO_A98_RGB_LINEAR: Mat3 = [
     1.38965124815152000, -0.16945907691487766, -0.22019217123664242,
    -0.22882573163305037,  1.23174254119010480, -0.00291680955705449,
    -0.01762544368426068, -0.09625702306122665,  1.11388246674548740,
];

// Rec2020 linear ↔ ProPhoto RGB linear
const REC2020_LINEAR_TO_PROPHOTO_RGB_LINEAR: Mat3 = [
    0.83518733312972350, 0.04886884858605698, 0.11594381828421951,
    0.05403324519953363, 0.92891840856920440, 0.01704834623126199,
   -0.00234203897072539, 0.03633215316169465, 0.96600988580903070,
];
const PROPHOTO_RGB_LINEAR_TO_REC2020_LINEAR: Mat3 = [
    1.20065932951740800, -0.05756805370122346, -0.14309127581618444,
   -0.06994154955888504,  1.08061789759721400, -0.01067634803832895,
    0.00554147334294746, -0.04078219298657951,  1.03524071964363200,
];

// ---- Direct linear ↔ XYZ-D50 matrices ----
// Used for lab/lch conversions to avoid XYZ-D65 → XYZ-D50 extra multiply

const SRGB_LINEAR_TO_XYZ_D50: Mat3 = [
    0.43606574687426936, 0.38515150959015960, 0.14307841996513868,
    0.22249317711056518, 0.71688701309448240, 0.06061980979495235,
    0.01392392146316939, 0.09708132423141015, 0.71409935681588070,
];
const XYZ_D50_TO_SRGB_LINEAR: Mat3 = [
     3.13413585290011780, -1.61738599801804200, -0.49066221791109754,
    -0.97879547655577770,  1.91625437739598840,  0.03344287339036693,
     0.07195539255794733, -0.22897675981518200,  1.40538603511311820,
];

const DISPLAY_P3_LINEAR_TO_XYZ_D50: Mat3 = [
    0.51514644296811600, 0.29200998206385770, 0.15713925139759397,
    0.24120032212525520, 0.69222254113138180, 0.06657713674336294,
   -0.00105013914714014, 0.04187827018907460, 0.78427647146852570,
];
const XYZ_D50_TO_DISPLAY_P3_LINEAR: Mat3 = [
     2.40393412185549730, -0.99003044249559310, -0.39761363181465614,
    -0.84227001614546880,  1.79895801610670820,  0.01604562477090472,
     0.04819381686413303, -0.09738519815446048,  1.27367136933212730,
];

const A98_RGB_LINEAR_TO_XYZ_D50: Mat3 = [
    0.60977504188618140, 0.20530000261929401, 0.14922063192409227,
    0.31112461220464155, 0.62565323083468560, 0.06322215696067286,
    0.01947059555648168, 0.06087908649415867, 0.74475492045981980,
];
const XYZ_D50_TO_A98_RGB_LINEAR: Mat3 = [
     1.96246703637688060, -0.61074234048150730, -0.34135809808271540,
    -0.97879547655577770,  1.91625437739598840,  0.03344287339036693,
     0.02870443944957101, -0.14067486633170680,  1.34891418141379370,
];

const REC2020_LINEAR_TO_XYZ_D50: Mat3 = [
    0.67351546318827600, 0.16569726370390453, 0.12508294953738705,
    0.27905900514112060, 0.67531800574910980, 0.04562298910976962,
   -0.00193242713400438, 0.02997782679282923, 0.79705920285163550,
];
const XYZ_D50_TO_REC2020_LINEAR: Mat3 = [
     1.64718490467176600, -0.39368189813164710, -0.23595963848828266,
    -0.68266410741738180,  1.64771461274440760,  0.01281708338512084,
     0.02966887665275675, -0.06292589642970030,  1.25355782018657710,
];

// ---- Direct linear ↔ LMS matrices (for OKLab) ----

const SRGB_LINEAR_TO_LMS: Mat3 = [
    0.41222146947076300, 0.53633253726173480, 0.05144599326750220,
    0.21190349581782520, 0.68069955064523420, 0.10739695353694050,
    0.08830245919005641, 0.28171883913612150, 0.62997870167382210,
];
const LMS_TO_SRGB_LINEAR: Mat3 = [
     4.07674163607595800, -3.30771153925806200, 0.23096990318210417,
    -1.26843797328503200,  2.60975734928768900, -0.34131937600265710,
    -0.00419607613867551, -0.70341861793593630, 1.70761469407461200,
];

const DISPLAY_P3_LINEAR_TO_LMS: Mat3 = [
    0.48137985274995443, 0.46211837101131803, 0.05650177623872756,
    0.22883194181124472, 0.65321681938356760, 0.11795123880518774,
    0.08394575232299319, 0.22416527097756642, 0.69188897669944040,
];
const LMS_TO_DISPLAY_P3_LINEAR: Mat3 = [
     3.12776897136187370, -2.25713576259163860, 0.12936679122976494,
    -1.09100901843779790,  2.41333171030692250, -0.32232269186912466,
    -0.02601080193857045, -0.50804133170416700, 1.53405213364273730,
];

const A98_RGB_LINEAR_TO_LMS: Mat3 = [
    0.57643225961839410, 0.36991322261987963, 0.05365451776172635,
    0.29631647054222465, 0.59167613325218850, 0.11200739620558686,
    0.12347825101427760, 0.21949869837199862, 0.65702305061372380,
];
const LMS_TO_A98_RGB_LINEAR: Mat3 = [
     2.55403683861155660, -1.62197618068286990, 0.06793934207131327,
    -1.26843797328503200,  2.60975734928768900, -0.34131937600265710,
    -0.05623473593749381, -0.56704183956690610, 1.62327657550439990,
];

const REC2020_LINEAR_TO_LMS: Mat3 = [
    0.61675578486544440, 0.36019840122646335, 0.02304581390809228,
    0.26513305939263670, 0.63583937206784910, 0.09902756853951408,
    0.10010262952034828, 0.20390652261661452, 0.69599084786303720,
];
const LMS_TO_REC2020_LINEAR: Mat3 = [
     2.13990673043465130, -1.24638949376061800, 0.10648276332596668,
    -0.88473583575776740,  2.16323093836120070, -0.27849510260343340,
    -0.04857374640044396, -0.45450314971409640, 1.50307689611454040,
];

const PROPHOTO_RGB_LINEAR_TO_LMS: Mat3 = [
    0.71544846056555340, 0.35279155007721186, -0.06824001064276530,
    0.27441164900156710, 0.66779764984123670, 0.05779070115719616,
    0.10978443261622942, 0.18619829115002018, 0.70401727623375040,
];
const LMS_TO_PROPHOTO_RGB_LINEAR: Mat3 = [
     1.73835514811572070, -0.98795094275144580, 0.24959579463572504,
    -0.70704940153292660,  1.93437004444013820, -0.22732064290721150,
    -0.08407882206239634, -0.35754060521141334, 1.44161942727380970,
];

// ---- OKLab matrices ----
// These are from Bjorn Ottosson's OKLab specification

const OKLAB_XYZ_TO_LMS: Mat3 = [
     0.81902243799670300, 0.36190626005289034, -0.12887378152098788,
     0.03298365393238846, 0.92928686158634330,  0.03614466635064235,
     0.04817718935962420, 0.26423953175273080,  0.63354782846943080,
];

const OKLAB_LMS_TO_XYZ: Mat3 = [
     1.22687987584592430, -0.55781499446021710,  0.28139104566596460,
    -0.04057574521480084,  1.11228680328031730, -0.07171105806551635,
    -0.07637293667466007, -0.42149333240224324,  1.58692401983678180,
];

const OKLAB_LMS_TO_OKLAB: Mat3 = [
    0.21045426830931400,  0.79361777470230540, -0.00407204301161930,
    1.97799853243116840, -2.42859224204858000,  0.45059370961741100,
    0.02590404246554780,  0.78277171245752960, -0.80867575492307740,
];

const OKLAB_OKLAB_TO_LMS: Mat3 = [
    1.00000000000000020,  0.39633777737617490,  0.21580375730991360,
    0.99999999999999980, -0.10556134581565854, -0.06385417282581334,
    0.99999999999999990, -0.08948417752981180, -1.29148554801940940,
];

// ---- Transfer functions (gamma encode/decode) ----

/// sRGB gamma encode (linear -> sRGB)
fn srgb_encode(c: f64) -> f64 {
    if c.abs() <= 0.0031308 {
        c * 12.92
    } else {
        c.signum() * (1.055 * c.abs().powf(1.0 / 2.4) - 0.055)
    }
}

/// sRGB gamma decode (sRGB -> linear)
fn srgb_decode(c: f64) -> f64 {
    if c.abs() <= 0.04045 {
        c / 12.92
    } else {
        c.signum() * ((c.abs() + 0.055) / 1.055).powf(2.4)
    }
}

/// Display P3 uses the same transfer function as sRGB
fn display_p3_encode(c: f64) -> f64 {
    srgb_encode(c)
}

fn display_p3_decode(c: f64) -> f64 {
    srgb_decode(c)
}

/// A98 RGB gamma encode
fn a98_rgb_encode(c: f64) -> f64 {
    c.signum() * c.abs().powf(256.0 / 563.0)
}

/// A98 RGB gamma decode
fn a98_rgb_decode(c: f64) -> f64 {
    c.signum() * c.abs().powf(563.0 / 256.0)
}

/// ProPhoto RGB gamma encode
fn prophoto_rgb_encode(c: f64) -> f64 {
    if c.abs() >= 1.0 / 512.0 {
        c.signum() * c.abs().powf(1.0 / 1.8)
    } else {
        c * 16.0
    }
}

/// ProPhoto RGB gamma decode
fn prophoto_rgb_decode(c: f64) -> f64 {
    if c.abs() >= 16.0 / 512.0 {
        c.signum() * c.abs().powf(1.8)
    } else {
        c / 16.0
    }
}

/// Rec2020 constants
const REC2020_ALPHA: f64 = 1.09929682680944;
const REC2020_BETA: f64 = 0.018053968510807;

/// Rec2020 gamma encode
fn rec2020_encode(c: f64) -> f64 {
    if c.abs() >= REC2020_BETA {
        c.signum() * (REC2020_ALPHA * c.abs().powf(0.45) - (REC2020_ALPHA - 1.0))
    } else {
        c * 4.5
    }
}

/// Rec2020 gamma decode
fn rec2020_decode(c: f64) -> f64 {
    if c.abs() >= REC2020_BETA * 4.5 {
        c.signum() * ((c.abs() + REC2020_ALPHA - 1.0) / REC2020_ALPHA).powf(1.0 / 0.45)
    } else {
        c / 4.5
    }
}

// ---- Non-linear color space conversions ----

/// Convert HSL to sRGB [0,1].
/// h in [0,360], s and l in [0,1].
pub fn hsl_to_srgb(h: f64, s: f64, l: f64) -> [f64; 3] {
    let h = h % 360.0;
    let scaled_hue = h / 360.0;

    let m2 = if l <= 0.5 {
        l * (s + 1.0)
    } else {
        l.mul_add(-s, l + s)
    };
    let m1 = l.mul_add(2.0, -m2);

    [
        hue_to_channel(m1, m2, scaled_hue + 1.0 / 3.0),
        hue_to_channel(m1, m2, scaled_hue),
        hue_to_channel(m1, m2, scaled_hue - 1.0 / 3.0),
    ]
}

fn hue_to_channel(m1: f64, m2: f64, mut hue: f64) -> f64 {
    if hue < 0.0 {
        hue += 1.0;
    }
    if hue > 1.0 {
        hue -= 1.0;
    }

    if hue < 1.0 / 6.0 {
        ((m2 - m1) * hue).mul_add(6.0, m1)
    } else if hue < 1.0 / 2.0 {
        m2
    } else if hue < 2.0 / 3.0 {
        ((m2 - m1) * (2.0 / 3.0 - hue)).mul_add(6.0, m1)
    } else {
        m1
    }
}

/// Convert sRGB to HSL.
/// Returns (hue, saturation, lightness). Handles out-of-gamut inputs
/// where values may be outside [0,1].
pub fn srgb_to_hsl(r: f64, g: f64, b: f64) -> [f64; 3] {
    let min = r.min(g.min(b));
    let max = r.max(g.max(b));
    let lightness = (min + max) / 2.0;

    // Use a tolerance larger than f64::EPSILON to handle floating-point noise
    // from color space conversion roundtrips (e.g., lab → xyz → srgb → hsl).
    if (max - min).abs() < 1e-10 {
        return [0.0, 0.0, lightness];
    }

    let delta = max - min;
    let sum = max + min;

    let denominator = if sum > 1.0 { 2.0 - sum } else { sum };
    // When lightness is 0 or 1 (denominator ≈ 0), saturation is 0
    let mut saturation = if denominator.abs() < f64::EPSILON {
        0.0
    } else {
        delta / denominator
    };

    let mut hue = if (max - b).abs() < f64::EPSILON && max != r {
        4.0 + (r - g) / delta
    } else if (max - g).abs() < f64::EPSILON {
        2.0 + (b - r) / delta
    } else {
        (g - b) / delta
    };

    hue *= 60.0;

    // For out-of-gamut values, saturation can come out negative.
    // Normalize by flipping sign and rotating hue by 180°.
    if saturation < 0.0 {
        saturation = -saturation;
        hue += 180.0;
    }

    if hue < 0.0 {
        hue += 360.0;
    }

    [hue % 360.0, saturation, lightness]
}

/// Convert HWB to sRGB [0,1].
/// h in [0,360], w and b in [0,1].
pub fn hwb_to_srgb(h: f64, w: f64, b: f64) -> [f64; 3] {
    let mut white = w;
    let mut black = b;
    let sum = white + black;
    if sum > 1.0 {
        white /= sum;
        black /= sum;
    }

    let factor = 1.0 - white - black;
    let hue = (h % 360.0) / 360.0;

    [
        hue_to_channel(0.0, 1.0, hue + 1.0 / 3.0).mul_add(factor, white),
        hue_to_channel(0.0, 1.0, hue).mul_add(factor, white),
        hue_to_channel(0.0, 1.0, hue - 1.0 / 3.0).mul_add(factor, white),
    ]
}

/// Convert sRGB [0,1] to HWB.
/// Returns (hue [0,360], whiteness [0,1], blackness [0,1]).
pub fn srgb_to_hwb(r: f64, g: f64, b: f64) -> [f64; 3] {
    let hsl = srgb_to_hsl(r, g, b);
    let white = r.min(g.min(b));
    let black = 1.0 - r.max(g.max(b));
    [hsl[0], white, black]
}

/// Convert Lab to XYZ-D50.
pub fn lab_to_xyz_d50(l: f64, a: f64, b: f64) -> [f64; 3] {
    const KAPPA: f64 = 24389.0 / 27.0;   // 903.296...
    const EPSILON: f64 = 216.0 / 24389.0; // 0.008856...

    // D50 white point
    const D50_X: f64 = 0.3457 / 0.3585;
    const D50_Y: f64 = 1.0;
    const D50_Z: f64 = (1.0 - 0.3457 - 0.3585) / 0.3585;

    let f1 = (l + 16.0) / 116.0;
    let f0 = a / 500.0 + f1;
    let f2 = f1 - b / 200.0;

    let x = if f0.powi(3) > EPSILON {
        f0.powi(3)
    } else {
        (116.0 * f0 - 16.0) / KAPPA
    };

    let y = if l > KAPPA * EPSILON {
        ((l + 16.0) / 116.0).powi(3)
    } else {
        l / KAPPA
    };

    let z = if f2.powi(3) > EPSILON {
        f2.powi(3)
    } else {
        (116.0 * f2 - 16.0) / KAPPA
    };

    [x * D50_X, y * D50_Y, z * D50_Z]
}

/// Convert XYZ-D50 to Lab.
pub fn xyz_d50_to_lab(x: f64, y: f64, z: f64) -> [f64; 3] {
    const KAPPA: f64 = 24389.0 / 27.0;
    const EPSILON: f64 = 216.0 / 24389.0;

    const D50_X: f64 = 0.3457 / 0.3585;
    const D50_Y: f64 = 1.0;
    const D50_Z: f64 = (1.0 - 0.3457 - 0.3585) / 0.3585;

    let f = |v: f64| -> f64 {
        if v > EPSILON {
            v.cbrt()
        } else {
            (KAPPA * v + 16.0) / 116.0
        }
    };

    let fx = f(x / D50_X);
    let fy = f(y / D50_Y);
    let fz = f(z / D50_Z);

    let l = 116.0 * fy - 16.0;
    let a = 500.0 * (fx - fy);
    let b = 200.0 * (fy - fz);

    [l, a, b]
}

/// Convert LCH to Lab.
pub fn lch_to_lab(l: f64, c: f64, h: f64) -> [f64; 3] {
    let h_rad = h * PI / 180.0;
    [l, c * h_rad.cos(), c * h_rad.sin()]
}

/// Convert Lab to LCH.
pub fn lab_to_lch(l: f64, a: f64, b: f64) -> [f64; 3] {
    let c = (a * a + b * b).sqrt();
    let mut h = b.atan2(a) * 180.0 / PI;
    if h < 0.0 {
        h += 360.0;
    }
    [l, c, h]
}

/// Convert OKLab to XYZ-D65.
pub fn oklab_to_xyz_d65(l: f64, a: f64, b: f64) -> [f64; 3] {
    // OKLab -> LMS (cube roots)
    let lms_g = mat3_mul(&OKLAB_OKLAB_TO_LMS, [l, a, b]);
    // Undo cube root
    let lms = [lms_g[0].powi(3), lms_g[1].powi(3), lms_g[2].powi(3)];
    // LMS -> XYZ-D65
    mat3_mul(&OKLAB_LMS_TO_XYZ, lms)
}

/// Convert XYZ-D65 to OKLab.
pub fn xyz_d65_to_oklab(x: f64, y: f64, z: f64) -> [f64; 3] {
    // XYZ-D65 -> LMS
    let lms = mat3_mul(&OKLAB_XYZ_TO_LMS, [x, y, z]);
    // Cube root
    let lms_g = [lms[0].cbrt(), lms[1].cbrt(), lms[2].cbrt()];
    // LMS -> OKLab
    mat3_mul(&OKLAB_LMS_TO_OKLAB, lms_g)
}

/// Convert OKLch to OKLab.
pub fn oklch_to_oklab(l: f64, c: f64, h: f64) -> [f64; 3] {
    lch_to_lab(l, c, h)
}

/// Convert OKLab to OKLch.
pub fn oklab_to_oklch(l: f64, a: f64, b: f64) -> [f64; 3] {
    lab_to_lch(l, a, b)
}

// ---- Direct linear-to-linear routing ----

/// The 5 RGB-family linear spaces. Used to index into the direct conversion
/// matrix table to avoid XYZ roundtrip precision loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinearRgbSpace {
    Srgb,
    DisplayP3,
    A98Rgb,
    Rec2020,
    ProphotoRgb,
}

/// Get the linear RGB space family for a color space, if it belongs to one.
/// Returns the linear space and a function to convert to/from it.
fn linear_rgb_space(space: ColorSpace) -> Option<LinearRgbSpace> {
    match space {
        ColorSpace::SRgb
        | ColorSpace::SRgbLinear
        | ColorSpace::Rgb
        | ColorSpace::Hsl
        | ColorSpace::Hwb => Some(LinearRgbSpace::Srgb),
        ColorSpace::DisplayP3 | ColorSpace::DisplayP3Linear => Some(LinearRgbSpace::DisplayP3),
        ColorSpace::A98Rgb => Some(LinearRgbSpace::A98Rgb),
        ColorSpace::Rec2020 => Some(LinearRgbSpace::Rec2020),
        ColorSpace::ProphotoRgb => Some(LinearRgbSpace::ProphotoRgb),
        _ => None,
    }
}

/// Convert from a color space to its linear RGB form.
fn to_linear_rgb(channels: [f64; 3], space: ColorSpace) -> [f64; 3] {
    let [c0, c1, c2] = channels;
    match space {
        ColorSpace::SRgbLinear => channels,
        ColorSpace::SRgb => [srgb_decode(c0), srgb_decode(c1), srgb_decode(c2)],
        ColorSpace::Rgb => {
            let s = [c0 / 255.0, c1 / 255.0, c2 / 255.0];
            [srgb_decode(s[0]), srgb_decode(s[1]), srgb_decode(s[2])]
        }
        ColorSpace::Hsl => {
            let s = hsl_to_srgb(c0, c1, c2);
            [srgb_decode(s[0]), srgb_decode(s[1]), srgb_decode(s[2])]
        }
        ColorSpace::Hwb => {
            let s = hwb_to_srgb(c0, c1, c2);
            [srgb_decode(s[0]), srgb_decode(s[1]), srgb_decode(s[2])]
        }
        ColorSpace::DisplayP3 => [
            display_p3_decode(c0),
            display_p3_decode(c1),
            display_p3_decode(c2),
        ],
        ColorSpace::DisplayP3Linear => channels,
        ColorSpace::A98Rgb => [a98_rgb_decode(c0), a98_rgb_decode(c1), a98_rgb_decode(c2)],
        ColorSpace::Rec2020 => [rec2020_decode(c0), rec2020_decode(c1), rec2020_decode(c2)],
        ColorSpace::ProphotoRgb => [
            prophoto_rgb_decode(c0),
            prophoto_rgb_decode(c1),
            prophoto_rgb_decode(c2),
        ],
        _ => unreachable!(),
    }
}

/// Convert from a linear RGB form to the target color space.
fn from_linear_rgb(linear: [f64; 3], space: ColorSpace) -> [f64; 3] {
    match space {
        ColorSpace::SRgbLinear => linear,
        ColorSpace::SRgb => [
            srgb_encode(linear[0]),
            srgb_encode(linear[1]),
            srgb_encode(linear[2]),
        ],
        ColorSpace::Rgb => {
            let s = [
                srgb_encode(linear[0]),
                srgb_encode(linear[1]),
                srgb_encode(linear[2]),
            ];
            [s[0] * 255.0, s[1] * 255.0, s[2] * 255.0]
        }
        ColorSpace::Hsl => {
            let s = [
                srgb_encode(linear[0]),
                srgb_encode(linear[1]),
                srgb_encode(linear[2]),
            ];
            srgb_to_hsl(s[0], s[1], s[2])
        }
        ColorSpace::Hwb => {
            let s = [
                srgb_encode(linear[0]),
                srgb_encode(linear[1]),
                srgb_encode(linear[2]),
            ];
            srgb_to_hwb(s[0], s[1], s[2])
        }
        ColorSpace::DisplayP3 => [
            display_p3_encode(linear[0]),
            display_p3_encode(linear[1]),
            display_p3_encode(linear[2]),
        ],
        ColorSpace::DisplayP3Linear => linear,
        ColorSpace::A98Rgb => [
            a98_rgb_encode(linear[0]),
            a98_rgb_encode(linear[1]),
            a98_rgb_encode(linear[2]),
        ],
        ColorSpace::Rec2020 => [
            rec2020_encode(linear[0]),
            rec2020_encode(linear[1]),
            rec2020_encode(linear[2]),
        ],
        ColorSpace::ProphotoRgb => [
            prophoto_rgb_encode(linear[0]),
            prophoto_rgb_encode(linear[1]),
            prophoto_rgb_encode(linear[2]),
        ],
        _ => unreachable!(),
    }
}

/// Direct matrix multiply between two different linear RGB spaces.
fn convert_between_linear_spaces(
    linear: [f64; 3],
    from: LinearRgbSpace,
    to: LinearRgbSpace,
) -> [f64; 3] {
    use LinearRgbSpace::*;
    let matrix = match (from, to) {
        (Srgb, DisplayP3) => &SRGB_LINEAR_TO_DISPLAY_P3_LINEAR,
        (DisplayP3, Srgb) => &DISPLAY_P3_LINEAR_TO_SRGB_LINEAR,
        (Srgb, A98Rgb) => &SRGB_LINEAR_TO_A98_RGB_LINEAR,
        (A98Rgb, Srgb) => &A98_RGB_LINEAR_TO_SRGB_LINEAR,
        (Srgb, Rec2020) => &SRGB_LINEAR_TO_REC2020_LINEAR,
        (Rec2020, Srgb) => &REC2020_LINEAR_TO_SRGB_LINEAR,
        (Srgb, ProphotoRgb) => &SRGB_LINEAR_TO_PROPHOTO_RGB_LINEAR,
        (ProphotoRgb, Srgb) => &PROPHOTO_RGB_LINEAR_TO_SRGB_LINEAR,
        (DisplayP3, A98Rgb) => &DISPLAY_P3_LINEAR_TO_A98_RGB_LINEAR,
        (A98Rgb, DisplayP3) => &A98_RGB_LINEAR_TO_DISPLAY_P3_LINEAR,
        (DisplayP3, Rec2020) => &DISPLAY_P3_LINEAR_TO_REC2020_LINEAR,
        (Rec2020, DisplayP3) => &REC2020_LINEAR_TO_DISPLAY_P3_LINEAR,
        (DisplayP3, ProphotoRgb) => &DISPLAY_P3_LINEAR_TO_PROPHOTO_RGB_LINEAR,
        (ProphotoRgb, DisplayP3) => &PROPHOTO_RGB_LINEAR_TO_DISPLAY_P3_LINEAR,
        (A98Rgb, Rec2020) => &A98_RGB_LINEAR_TO_REC2020_LINEAR,
        (Rec2020, A98Rgb) => &REC2020_LINEAR_TO_A98_RGB_LINEAR,
        (A98Rgb, ProphotoRgb) => &A98_RGB_LINEAR_TO_PROPHOTO_RGB_LINEAR,
        (ProphotoRgb, A98Rgb) => &PROPHOTO_RGB_LINEAR_TO_A98_RGB_LINEAR,
        (Rec2020, ProphotoRgb) => &REC2020_LINEAR_TO_PROPHOTO_RGB_LINEAR,
        (ProphotoRgb, Rec2020) => &PROPHOTO_RGB_LINEAR_TO_REC2020_LINEAR,
        _ => unreachable!("same space should be handled before calling this"),
    };
    mat3_mul(matrix, linear)
}

/// Convert from a linear RGB space directly to XYZ-D50 (for lab/lch).
fn linear_rgb_to_xyz_d50(linear: [f64; 3], from: LinearRgbSpace) -> [f64; 3] {
    use LinearRgbSpace::*;
    let matrix = match from {
        Srgb => &SRGB_LINEAR_TO_XYZ_D50,
        DisplayP3 => &DISPLAY_P3_LINEAR_TO_XYZ_D50,
        A98Rgb => &A98_RGB_LINEAR_TO_XYZ_D50,
        Rec2020 => &REC2020_LINEAR_TO_XYZ_D50,
        ProphotoRgb => &PROPHOTO_RGB_LINEAR_TO_XYZ_D50,
    };
    mat3_mul(matrix, linear)
}

/// Convert from XYZ-D50 directly to a linear RGB space (for lab/lch).
fn xyz_d50_to_linear_rgb(xyz_d50: [f64; 3], to: LinearRgbSpace) -> [f64; 3] {
    use LinearRgbSpace::*;
    let matrix = match to {
        Srgb => &XYZ_D50_TO_SRGB_LINEAR,
        DisplayP3 => &XYZ_D50_TO_DISPLAY_P3_LINEAR,
        A98Rgb => &XYZ_D50_TO_A98_RGB_LINEAR,
        Rec2020 => &XYZ_D50_TO_REC2020_LINEAR,
        ProphotoRgb => &XYZ_D50_TO_PROPHOTO_RGB_LINEAR,
    };
    mat3_mul(matrix, xyz_d50)
}

/// Convert from a linear RGB space directly to LMS (for oklab/oklch).
fn linear_rgb_to_lms(linear: [f64; 3], from: LinearRgbSpace) -> [f64; 3] {
    use LinearRgbSpace::*;
    let matrix = match from {
        Srgb => &SRGB_LINEAR_TO_LMS,
        DisplayP3 => &DISPLAY_P3_LINEAR_TO_LMS,
        A98Rgb => &A98_RGB_LINEAR_TO_LMS,
        Rec2020 => &REC2020_LINEAR_TO_LMS,
        ProphotoRgb => &PROPHOTO_RGB_LINEAR_TO_LMS,
    };
    mat3_mul(matrix, linear)
}

/// Convert from LMS directly to a linear RGB space (for oklab/oklch).
fn lms_to_linear_rgb(lms: [f64; 3], to: LinearRgbSpace) -> [f64; 3] {
    use LinearRgbSpace::*;
    let matrix = match to {
        Srgb => &LMS_TO_SRGB_LINEAR,
        DisplayP3 => &LMS_TO_DISPLAY_P3_LINEAR,
        A98Rgb => &LMS_TO_A98_RGB_LINEAR,
        Rec2020 => &LMS_TO_REC2020_LINEAR,
        ProphotoRgb => &LMS_TO_PROPHOTO_RGB_LINEAR,
    };
    mat3_mul(matrix, lms)
}

// ---- High-level conversion: any space -> any space ----

/// Convert a color from one space to another.
///
/// `channels` are the 3 channel values in the source space.
/// Missing channels (None) are treated as 0 for conversion purposes,
/// as specified by CSS Color Level 4.
pub fn convert(
    channels: [f64; 3],
    from: ColorSpace,
    to: ColorSpace,
) -> [f64; 3] {
    if from == to {
        return channels;
    }

    // Direct shortcuts for same-family conversions (e.g. sRGB↔HSL)
    if let Some(result) = convert_direct(channels, from, to) {
        return result;
    }

    // Try direct linear-to-linear path for RGB-family spaces.
    // This uses pre-composed matrices to avoid XYZ roundtrip precision loss.
    let from_linear = linear_rgb_space(from);
    let to_linear = linear_rgb_space(to);

    match (from_linear, to_linear) {
        // Both are RGB-family: go source → linear → (direct matrix) → linear → target
        (Some(from_ls), Some(to_ls)) => {
            let linear = to_linear_rgb(channels, from);
            let target_linear = if from_ls == to_ls {
                linear
            } else {
                convert_between_linear_spaces(linear, from_ls, to_ls)
            };
            from_linear_rgb(target_linear, to)
        }

        // Source is RGB-family, target is Lab/LCH: use direct XYZ-D50 matrix
        (Some(from_ls), None) if to == ColorSpace::Lab || to == ColorSpace::Lch => {
            let linear = to_linear_rgb(channels, from);
            let xyz_d50 = linear_rgb_to_xyz_d50(linear, from_ls);
            let lab = xyz_d50_to_lab(xyz_d50[0], xyz_d50[1], xyz_d50[2]);
            if to == ColorSpace::Lch {
                lab_to_lch(lab[0], lab[1], lab[2])
            } else {
                lab
            }
        }

        // Source is Lab/LCH, target is RGB-family: use direct XYZ-D50 matrix
        (None, Some(to_ls)) if from == ColorSpace::Lab || from == ColorSpace::Lch => {
            let [c0, c1, c2] = channels;
            let lab = if from == ColorSpace::Lch {
                lch_to_lab(c0, c1, c2)
            } else {
                channels
            };
            let xyz_d50 = lab_to_xyz_d50(lab[0], lab[1], lab[2]);
            let linear = xyz_d50_to_linear_rgb(xyz_d50, to_ls);
            from_linear_rgb(linear, to)
        }

        // Source is RGB-family, target is OKLab/OKLch: use direct LMS matrix
        (Some(from_ls), None)
            if to == ColorSpace::Oklab || to == ColorSpace::Oklch =>
        {
            let linear = to_linear_rgb(channels, from);
            let lms = linear_rgb_to_lms(linear, from_ls);
            let lms_g = [lms[0].cbrt(), lms[1].cbrt(), lms[2].cbrt()];
            let oklab = mat3_mul(&OKLAB_LMS_TO_OKLAB, lms_g);
            if to == ColorSpace::Oklch {
                oklab_to_oklch(oklab[0], oklab[1], oklab[2])
            } else {
                oklab
            }
        }

        // Source is OKLab/OKLch, target is RGB-family: use direct LMS matrix
        (None, Some(to_ls))
            if from == ColorSpace::Oklab || from == ColorSpace::Oklch =>
        {
            let [c0, c1, c2] = channels;
            let oklab = if from == ColorSpace::Oklch {
                oklch_to_oklab(c0, c1, c2)
            } else {
                channels
            };
            let lms_g = mat3_mul(&OKLAB_OKLAB_TO_LMS, oklab);
            let lms = [lms_g[0].powi(3), lms_g[1].powi(3), lms_g[2].powi(3)];
            let linear = lms_to_linear_rgb(lms, to_ls);
            from_linear_rgb(linear, to)
        }

        // Source is RGB-family, target is XYZ-D50: use direct matrix
        (Some(from_ls), None) if to == ColorSpace::XyzD50 => {
            let linear = to_linear_rgb(channels, from);
            linear_rgb_to_xyz_d50(linear, from_ls)
        }

        // Source is XYZ-D50, target is RGB-family: use direct matrix
        (None, Some(to_ls)) if from == ColorSpace::XyzD50 => {
            let linear = xyz_d50_to_linear_rgb(channels, to_ls);
            from_linear_rgb(linear, to)
        }

        // Fallback: go through XYZ-D65 (for XYZ↔XYZ, Lab↔OKLab, etc.)
        _ => {
            let xyz_d65 = to_xyz_d65(channels, from);
            from_xyz_d65(xyz_d65, to)
        }
    }
}

/// Direct conversion shortcuts between related color spaces.
/// Returns None if no direct path exists.
fn convert_direct(channels: [f64; 3], from: ColorSpace, to: ColorSpace) -> Option<[f64; 3]> {
    let [c0, c1, c2] = channels;

    match (from, to) {
        // Legacy RGB ↔ sRGB
        (ColorSpace::Rgb, ColorSpace::SRgb) => Some([c0 / 255.0, c1 / 255.0, c2 / 255.0]),
        (ColorSpace::SRgb, ColorSpace::Rgb) => Some([c0 * 255.0, c1 * 255.0, c2 * 255.0]),

        // sRGB ↔ HSL
        (ColorSpace::SRgb, ColorSpace::Hsl) => Some(srgb_to_hsl(c0, c1, c2)),
        (ColorSpace::Hsl, ColorSpace::SRgb) => Some(hsl_to_srgb(c0, c1, c2)),

        // sRGB ↔ HWB
        (ColorSpace::SRgb, ColorSpace::Hwb) => Some(srgb_to_hwb(c0, c1, c2)),
        (ColorSpace::Hwb, ColorSpace::SRgb) => Some(hwb_to_srgb(c0, c1, c2)),

        // HSL ↔ HWB (via sRGB)
        (ColorSpace::Hsl, ColorSpace::Hwb) => {
            let srgb = hsl_to_srgb(c0, c1, c2);
            Some(srgb_to_hwb(srgb[0], srgb[1], srgb[2]))
        }
        (ColorSpace::Hwb, ColorSpace::Hsl) => {
            let srgb = hwb_to_srgb(c0, c1, c2);
            Some(srgb_to_hsl(srgb[0], srgb[1], srgb[2]))
        }

        // Legacy RGB ↔ HSL/HWB (via sRGB)
        (ColorSpace::Rgb, ColorSpace::Hsl) => Some(srgb_to_hsl(c0 / 255.0, c1 / 255.0, c2 / 255.0)),
        (ColorSpace::Hsl, ColorSpace::Rgb) => {
            let srgb = hsl_to_srgb(c0, c1, c2);
            Some([srgb[0] * 255.0, srgb[1] * 255.0, srgb[2] * 255.0])
        }
        (ColorSpace::Rgb, ColorSpace::Hwb) => Some(srgb_to_hwb(c0 / 255.0, c1 / 255.0, c2 / 255.0)),
        (ColorSpace::Hwb, ColorSpace::Rgb) => {
            let srgb = hwb_to_srgb(c0, c1, c2);
            Some([srgb[0] * 255.0, srgb[1] * 255.0, srgb[2] * 255.0])
        }

        // sRGB ↔ sRGB-linear (gamma encode/decode)
        (ColorSpace::SRgb, ColorSpace::SRgbLinear) => {
            Some([srgb_decode(c0), srgb_decode(c1), srgb_decode(c2)])
        }
        (ColorSpace::SRgbLinear, ColorSpace::SRgb) => {
            Some([srgb_encode(c0), srgb_encode(c1), srgb_encode(c2)])
        }

        // Lab ↔ LCH
        (ColorSpace::Lab, ColorSpace::Lch) => Some(lab_to_lch(c0, c1, c2)),
        (ColorSpace::Lch, ColorSpace::Lab) => Some(lch_to_lab(c0, c1, c2)),

        // OKLab ↔ OKLch
        (ColorSpace::Oklab, ColorSpace::Oklch) => Some(oklab_to_oklch(c0, c1, c2)),
        (ColorSpace::Oklch, ColorSpace::Oklab) => Some(oklch_to_oklab(c0, c1, c2)),

        // DisplayP3 ↔ DisplayP3Linear
        (ColorSpace::DisplayP3, ColorSpace::DisplayP3Linear) => {
            Some([display_p3_decode(c0), display_p3_decode(c1), display_p3_decode(c2)])
        }
        (ColorSpace::DisplayP3Linear, ColorSpace::DisplayP3) => {
            Some([display_p3_encode(c0), display_p3_encode(c1), display_p3_encode(c2)])
        }

        _ => None,
    }
}

/// Convert from any space to XYZ-D65.
fn to_xyz_d65(channels: [f64; 3], space: ColorSpace) -> [f64; 3] {
    let [c0, c1, c2] = channels;

    match space {
        ColorSpace::XyzD65 => channels,

        ColorSpace::XyzD50 => mat3_mul(&XYZ_D50_TO_XYZ_D65, channels),

        ColorSpace::SRgbLinear => mat3_mul(&SRGB_LINEAR_TO_XYZ_D65, channels),

        ColorSpace::SRgb => {
            let linear = [srgb_decode(c0), srgb_decode(c1), srgb_decode(c2)];
            mat3_mul(&SRGB_LINEAR_TO_XYZ_D65, linear)
        }

        ColorSpace::Rgb => {
            // Legacy RGB [0,255] -> sRGB [0,1] -> linear -> XYZ
            let srgb = [c0 / 255.0, c1 / 255.0, c2 / 255.0];
            let linear = [srgb_decode(srgb[0]), srgb_decode(srgb[1]), srgb_decode(srgb[2])];
            mat3_mul(&SRGB_LINEAR_TO_XYZ_D65, linear)
        }

        ColorSpace::Hsl => {
            let srgb = hsl_to_srgb(c0, c1, c2);
            let linear = [srgb_decode(srgb[0]), srgb_decode(srgb[1]), srgb_decode(srgb[2])];
            mat3_mul(&SRGB_LINEAR_TO_XYZ_D65, linear)
        }

        ColorSpace::Hwb => {
            let srgb = hwb_to_srgb(c0, c1, c2);
            let linear = [srgb_decode(srgb[0]), srgb_decode(srgb[1]), srgb_decode(srgb[2])];
            mat3_mul(&SRGB_LINEAR_TO_XYZ_D65, linear)
        }

        ColorSpace::DisplayP3 => {
            let linear = [display_p3_decode(c0), display_p3_decode(c1), display_p3_decode(c2)];
            mat3_mul(&DISPLAY_P3_LINEAR_TO_XYZ_D65, linear)
        }

        ColorSpace::DisplayP3Linear => mat3_mul(&DISPLAY_P3_LINEAR_TO_XYZ_D65, channels),

        ColorSpace::A98Rgb => {
            let linear = [a98_rgb_decode(c0), a98_rgb_decode(c1), a98_rgb_decode(c2)];
            mat3_mul(&A98_RGB_LINEAR_TO_XYZ_D65, linear)
        }

        ColorSpace::ProphotoRgb => {
            let linear = [prophoto_rgb_decode(c0), prophoto_rgb_decode(c1), prophoto_rgb_decode(c2)];
            let xyz_d50 = mat3_mul(&PROPHOTO_RGB_LINEAR_TO_XYZ_D50, linear);
            mat3_mul(&XYZ_D50_TO_XYZ_D65, xyz_d50)
        }

        ColorSpace::Rec2020 => {
            let linear = [rec2020_decode(c0), rec2020_decode(c1), rec2020_decode(c2)];
            mat3_mul(&REC2020_LINEAR_TO_XYZ_D65, linear)
        }

        ColorSpace::Lab => {
            let xyz_d50 = lab_to_xyz_d50(c0, c1, c2);
            mat3_mul(&XYZ_D50_TO_XYZ_D65, xyz_d50)
        }

        ColorSpace::Lch => {
            let lab = lch_to_lab(c0, c1, c2);
            let xyz_d50 = lab_to_xyz_d50(lab[0], lab[1], lab[2]);
            mat3_mul(&XYZ_D50_TO_XYZ_D65, xyz_d50)
        }

        ColorSpace::Oklab => oklab_to_xyz_d65(c0, c1, c2),

        ColorSpace::Oklch => {
            let oklab = oklch_to_oklab(c0, c1, c2);
            oklab_to_xyz_d65(oklab[0], oklab[1], oklab[2])
        }
    }
}

/// Convert from XYZ-D65 to any space.
fn from_xyz_d65(xyz: [f64; 3], space: ColorSpace) -> [f64; 3] {
    match space {
        ColorSpace::XyzD65 => xyz,

        ColorSpace::XyzD50 => mat3_mul(&XYZ_D65_TO_XYZ_D50, xyz),

        ColorSpace::SRgbLinear => mat3_mul(&XYZ_D65_TO_SRGB_LINEAR, xyz),

        ColorSpace::SRgb => {
            let linear = mat3_mul(&XYZ_D65_TO_SRGB_LINEAR, xyz);
            [srgb_encode(linear[0]), srgb_encode(linear[1]), srgb_encode(linear[2])]
        }

        ColorSpace::Rgb => {
            let linear = mat3_mul(&XYZ_D65_TO_SRGB_LINEAR, xyz);
            let srgb = [srgb_encode(linear[0]), srgb_encode(linear[1]), srgb_encode(linear[2])];
            [srgb[0] * 255.0, srgb[1] * 255.0, srgb[2] * 255.0]
        }

        ColorSpace::Hsl => {
            let linear = mat3_mul(&XYZ_D65_TO_SRGB_LINEAR, xyz);
            let srgb = [srgb_encode(linear[0]), srgb_encode(linear[1]), srgb_encode(linear[2])];
            srgb_to_hsl(srgb[0], srgb[1], srgb[2])
        }

        ColorSpace::Hwb => {
            let linear = mat3_mul(&XYZ_D65_TO_SRGB_LINEAR, xyz);
            let srgb = [srgb_encode(linear[0]), srgb_encode(linear[1]), srgb_encode(linear[2])];
            srgb_to_hwb(srgb[0], srgb[1], srgb[2])
        }

        ColorSpace::DisplayP3 => {
            let linear = mat3_mul(&XYZ_D65_TO_DISPLAY_P3_LINEAR, xyz);
            [display_p3_encode(linear[0]), display_p3_encode(linear[1]), display_p3_encode(linear[2])]
        }

        ColorSpace::DisplayP3Linear => mat3_mul(&XYZ_D65_TO_DISPLAY_P3_LINEAR, xyz),

        ColorSpace::A98Rgb => {
            let linear = mat3_mul(&XYZ_D65_TO_A98_RGB_LINEAR, xyz);
            [a98_rgb_encode(linear[0]), a98_rgb_encode(linear[1]), a98_rgb_encode(linear[2])]
        }

        ColorSpace::ProphotoRgb => {
            let xyz_d50 = mat3_mul(&XYZ_D65_TO_XYZ_D50, xyz);
            let linear = mat3_mul(&XYZ_D50_TO_PROPHOTO_RGB_LINEAR, xyz_d50);
            [prophoto_rgb_encode(linear[0]), prophoto_rgb_encode(linear[1]), prophoto_rgb_encode(linear[2])]
        }

        ColorSpace::Rec2020 => {
            let linear = mat3_mul(&XYZ_D65_TO_REC2020_LINEAR, xyz);
            [rec2020_encode(linear[0]), rec2020_encode(linear[1]), rec2020_encode(linear[2])]
        }

        ColorSpace::Lab => {
            let xyz_d50 = mat3_mul(&XYZ_D65_TO_XYZ_D50, xyz);
            xyz_d50_to_lab(xyz_d50[0], xyz_d50[1], xyz_d50[2])
        }

        ColorSpace::Lch => {
            let xyz_d50 = mat3_mul(&XYZ_D65_TO_XYZ_D50, xyz);
            let lab = xyz_d50_to_lab(xyz_d50[0], xyz_d50[1], xyz_d50[2]);
            lab_to_lch(lab[0], lab[1], lab[2])
        }

        ColorSpace::Oklab => xyz_d65_to_oklab(xyz[0], xyz[1], xyz[2]),

        ColorSpace::Oklch => {
            let oklab = xyz_d65_to_oklab(xyz[0], xyz[1], xyz[2]);
            oklab_to_oklch(oklab[0], oklab[1], oklab[2])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_approx(actual: [f64; 3], expected: [f64; 3], tolerance: f64) {
        for i in 0..3 {
            assert!(
                (actual[i] - expected[i]).abs() < tolerance,
                "channel {}: expected {}, got {} (diff {})",
                i,
                expected[i],
                actual[i],
                (actual[i] - expected[i]).abs()
            );
        }
    }

    #[test]
    fn rgb_to_hsl_roundtrip() {
        // Pure red
        let hsl = srgb_to_hsl(1.0, 0.0, 0.0);
        assert_approx(hsl, [0.0, 1.0, 0.5], 1e-10);

        let rgb = hsl_to_srgb(hsl[0], hsl[1], hsl[2]);
        assert_approx(rgb, [1.0, 0.0, 0.0], 1e-10);
    }

    #[test]
    fn rgb_to_hwb_roundtrip() {
        // Pure green
        let hwb = srgb_to_hwb(0.0, 1.0, 0.0);
        assert_approx(hwb, [120.0, 0.0, 0.0], 1e-10);

        let rgb = hwb_to_srgb(hwb[0], hwb[1], hwb[2]);
        assert_approx(rgb, [0.0, 1.0, 0.0], 1e-10);
    }

    #[test]
    fn srgb_to_lab_roundtrip() {
        // White
        let lab = convert([1.0, 1.0, 1.0], ColorSpace::SRgb, ColorSpace::Lab);
        assert_approx(lab, [100.0, 0.0, 0.0], 0.01);

        let srgb = convert(lab, ColorSpace::Lab, ColorSpace::SRgb);
        assert_approx(srgb, [1.0, 1.0, 1.0], 1e-6);
    }

    #[test]
    fn srgb_to_oklab_roundtrip() {
        // Red
        let oklab = convert([1.0, 0.0, 0.0], ColorSpace::SRgb, ColorSpace::Oklab);
        // Known values for red in OKLab (approx)
        assert!((oklab[0] - 0.6279553606).abs() < 0.001);

        let srgb = convert(oklab, ColorSpace::Oklab, ColorSpace::SRgb);
        assert_approx(srgb, [1.0, 0.0, 0.0], 1e-4);
    }

    #[test]
    fn srgb_to_display_p3_roundtrip() {
        let p3 = convert([1.0, 0.0, 0.0], ColorSpace::SRgb, ColorSpace::DisplayP3);
        let srgb = convert(p3, ColorSpace::DisplayP3, ColorSpace::SRgb);
        assert_approx(srgb, [1.0, 0.0, 0.0], 1e-6);
    }

    #[test]
    fn identity_conversion() {
        let channels = [0.5, 0.3, 0.8];
        assert_eq!(convert(channels, ColorSpace::SRgb, ColorSpace::SRgb), channels);
    }

    #[test]
    fn lch_lab_roundtrip() {
        let lab = [50.0, 30.0, -20.0];
        let lch = lab_to_lch(lab[0], lab[1], lab[2]);
        let back = lch_to_lab(lch[0], lch[1], lch[2]);
        assert_approx(back, lab, 1e-10);
    }

    #[test]
    fn oklch_oklab_roundtrip() {
        let oklab = [0.5, 0.1, -0.05];
        let oklch = oklab_to_oklch(oklab[0], oklab[1], oklab[2]);
        let back = oklch_to_oklab(oklch[0], oklch[1], oklch[2]);
        assert_approx(back, oklab, 1e-10);
    }

    #[test]
    fn legacy_rgb_to_srgb() {
        // Legacy RGB 255,0,0 -> sRGB 1,0,0
        let srgb = convert([255.0, 0.0, 0.0], ColorSpace::Rgb, ColorSpace::SRgb);
        assert_approx(srgb, [1.0, 0.0, 0.0], 1e-6);
    }

    #[test]
    fn prophoto_roundtrip() {
        let channels = [0.5, 0.3, 0.8];
        let xyz = convert(channels, ColorSpace::ProphotoRgb, ColorSpace::XyzD65);
        let back = convert(xyz, ColorSpace::XyzD65, ColorSpace::ProphotoRgb);
        assert_approx(back, channels, 1e-6);
    }

    #[test]
    fn rec2020_roundtrip() {
        let channels = [0.5, 0.3, 0.8];
        let xyz = convert(channels, ColorSpace::Rec2020, ColorSpace::XyzD65);
        let back = convert(xyz, ColorSpace::XyzD65, ColorSpace::Rec2020);
        assert_approx(back, channels, 1e-10);
    }

    #[test]
    fn a98_rgb_roundtrip() {
        let channels = [0.5, 0.3, 0.8];
        let xyz = convert(channels, ColorSpace::A98Rgb, ColorSpace::XyzD65);
        let back = convert(xyz, ColorSpace::XyzD65, ColorSpace::A98Rgb);
        assert_approx(back, channels, 1e-10);
    }
}
