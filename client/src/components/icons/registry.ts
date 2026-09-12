import type { IconCategory, IconFamily, IconSpec } from "./types";
import { ICON_FAMILIES } from "./types";

/**
 * Extension → drawing. This is data, not logic: one row per file type, and the
 * families in `families/` never look anything up themselves.
 *
 * Two rules hold the table together. First, *convention beats invention*: a
 * Word file is blue, Excel green, PowerPoint orange, a PDF red, Rust rust,
 * Go cyan — people recognise a file type by its color before they read the
 * monogram, so the registry borrows the color the world already agreed on.
 * Second, *every hue is tuned for #010513*: the app's ground is near-black, so
 * each pair is a lifted face over a plate at least a quarter darker, and the
 * yellows carry a deep amber second stop so a white monogram still reads.
 *
 * Neighbouring languages are pushed apart deliberately (`py` blue/gold, `ts`
 * blue, `js` yellow, `go` cyan, `rs` rust) because a folder of source files is
 * exactly where a grid of near-identical icons stops being useful.
 */

/** A face color and the darker plate under it. Authored as a pair so no row can half-tint. */
type Duo = readonly [hue: string, hue2: string];

/*
 * The palette. Twenty pairs, every `hue2` ≥ 25% darker than its `hue` in plain
 * sRGB so the extrusion never dissolves into the face, and nothing so dim that
 * it disappears against the near-black app background.
 */
const BLUE: Duo = ["#4C8DFF", "#2457C5"];
const SKY: Duo = ["#6FC3FF", "#2079C4"];
const INDIGO: Duo = ["#7C8CFF", "#3B47C9"];
const CYAN: Duo = ["#4FD3E8", "#1B8AA3"];
const TEAL: Duo = ["#4FD9C4", "#17948A"];
const MINT: Duo = ["#7FE3B0", "#2E9B6C"];
const GREEN: Duo = ["#5FD37A", "#218F45"];
const LIME: Duo = ["#B9E05A", "#6E8F14"];
const YELLOW: Duo = ["#F5D247", "#B58A00"];
const AMBER: Duo = ["#FFB74D", "#C2700A"];
const ORANGE: Duo = ["#FF9152", "#CC5314"];
const RUST: Duo = ["#E08A5F", "#A0522D"];
const BROWN: Duo = ["#B98A63", "#7A5433"];
const RED: Duo = ["#FF6E6E", "#C43A3A"];
const PINK: Duo = ["#FF8ED6", "#C44B95"];
const PURPLE: Duo = ["#A87BFF", "#6234C4"];
const VIOLET: Duo = ["#8A63FF", "#4E0EFF"];
const SLATE: Duo = ["#8A93A8", "#565E73"];
const GRAPHITE: Duo = ["#6B7386", "#414859"];
/** Python's own blue-over-gold, the one convention that wants a two-*family* pair. */
const PYTHON: Duo = ["#5FA8DC", "#B8891A"];

/** One row, written the way the table reads: what it is, what it says, how it is lit. */
const s = (family: IconFamily, label: string, [hue, hue2]: Duo, category: IconCategory): IconSpec => ({
  family,
  label,
  hue,
  hue2,
  category,
});

/** What an unrecognised file looks like: neutral graphite, no label. */
export const GENERIC_SPEC: IconSpec = {
  family: "generic",
  label: "",
  hue: "#6B7386",
  hue2: "#414859",
  category: "other",
};

/**
 * Null-prototype on purpose. File names come from disk, so a directory holding
 * `notes.constructor` or `x.toString` would otherwise resolve to an inherited
 * `Object.prototype` member — truthy, not an `IconSpec` — and take the drawing
 * code down with it. Nothing is inherited here, so an unknown extension is
 * genuinely `undefined` and the `GENERIC_SPEC` fallback fires.
 */
export const EXTENSION_SPECS: Record<string, IconSpec> = Object.assign(
  Object.create(null) as Record<string, IconSpec>,
  {
    // ── Word processing ──────────────────────────────────────────────────
    doc: s("document", "DOC", BLUE, "document"),
    docx: s("document", "DOCX", BLUE, "document"),
    dot: s("document", "DOT", BLUE, "document"),
    dotx: s("document", "DOTX", BLUE, "document"),
    odt: s("document", "ODT", INDIGO, "document"),
    rtf: s("document", "RTF", SKY, "document"),
    pages: s("document", "PAGE", ORANGE, "document"),
    wps: s("document", "WPS", BLUE, "document"),

    // ── Plain text & markup prose ────────────────────────────────────────
    txt: s("text", "TXT", SLATE, "document"),
    md: s("text", "MD", SKY, "document"),
    markdown: s("text", "MD", SKY, "document"),
    mdx: s("text", "MDX", SKY, "document"),
    rst: s("text", "RST", SLATE, "document"),
    tex: s("text", "TEX", TEAL, "document"),
    ltx: s("text", "LTX", TEAL, "document"),
    bib: s("text", "BIB", TEAL, "document"),
    log: s("text", "LOG", GRAPHITE, "document"),
    nfo: s("text", "NFO", GRAPHITE, "document"),
    readme: s("text", "READ", SKY, "document"),

    // ── Spreadsheets ─────────────────────────────────────────────────────
    xls: s("spreadsheet", "XLS", GREEN, "document"),
    xlsx: s("spreadsheet", "XLSX", GREEN, "document"),
    xlsm: s("spreadsheet", "XLSM", GREEN, "document"),
    xlsb: s("spreadsheet", "XLSB", GREEN, "document"),
    numbers: s("spreadsheet", "NUM", GREEN, "document"),
    ods: s("spreadsheet", "ODS", GREEN, "document"),
    csv: s("spreadsheet", "CSV", MINT, "data"),
    tsv: s("spreadsheet", "TSV", MINT, "data"),

    // ── Presentations ────────────────────────────────────────────────────
    ppt: s("presentation", "PPT", ORANGE, "document"),
    pptx: s("presentation", "PPTX", ORANGE, "document"),
    pps: s("presentation", "PPS", ORANGE, "document"),
    ppsx: s("presentation", "PPSX", ORANGE, "document"),
    key: s("presentation", "KEY", AMBER, "document"),
    odp: s("presentation", "ODP", ORANGE, "document"),

    // ── Portable documents & books ───────────────────────────────────────
    pdf: s("pdf", "PDF", RED, "document"),
    epub: s("book", "EPUB", PURPLE, "document"),
    mobi: s("book", "MOBI", PURPLE, "document"),
    azw: s("book", "AZW", AMBER, "document"),
    azw3: s("book", "AZW3", AMBER, "document"),
    djvu: s("book", "DJVU", TEAL, "document"),
    cbz: s("book", "CBZ", VIOLET, "document"),
    cbr: s("book", "CBR", VIOLET, "document"),
    fb2: s("book", "FB2", PURPLE, "document"),

    // ── Python & scientific computing ────────────────────────────────────
    py: s("code", "PY", PYTHON, "code"),
    pyc: s("code", "PYC", SLATE, "code"),
    pyi: s("code", "PYI", PYTHON, "code"),
    ipynb: s("code", "IPYN", ORANGE, "code"),
    r: s("code", "R", BLUE, "code"),
    rmd: s("code", "RMD", BLUE, "code"),
    jl: s("code", "JL", PURPLE, "code"),

    // ── TypeScript / JavaScript ──────────────────────────────────────────
    ts: s("code", "TS", BLUE, "code"),
    tsx: s("code", "TSX", BLUE, "code"),
    "d.ts": s("code", "DTS", INDIGO, "code"),
    "spec.ts": s("code", "SPEC", TEAL, "code"),
    "test.ts": s("code", "TEST", TEAL, "code"),
    js: s("code", "JS", YELLOW, "code"),
    jsx: s("code", "JSX", YELLOW, "code"),
    mjs: s("code", "MJS", YELLOW, "code"),
    cjs: s("code", "CJS", YELLOW, "code"),
    "min.js": s("code", "MIN", AMBER, "code"),

    // ── Systems & compiled languages ─────────────────────────────────────
    rs: s("code", "RS", RUST, "code"),
    go: s("code", "GO", CYAN, "code"),
    java: s("code", "JAVA", RED, "code"),
    kt: s("code", "KT", PURPLE, "code"),
    kts: s("code", "KTS", PURPLE, "code"),
    scala: s("code", "SCLA", RED, "code"),
    groovy: s("code", "GRVY", SKY, "code"),
    swift: s("code", "SWFT", ORANGE, "code"),
    m: s("code", "M", INDIGO, "code"),
    mm: s("code", "MM", INDIGO, "code"),
    c: s("code", "C", INDIGO, "code"),
    h: s("code", "H", INDIGO, "code"),
    cpp: s("code", "CPP", BLUE, "code"),
    cc: s("code", "CC", BLUE, "code"),
    cxx: s("code", "CXX", BLUE, "code"),
    hpp: s("code", "HPP", BLUE, "code"),
    hh: s("code", "HH", BLUE, "code"),
    cs: s("code", "CS", PURPLE, "code"),
    fs: s("code", "FS", CYAN, "code"),
    vb: s("code", "VB", INDIGO, "code"),
    dart: s("code", "DART", TEAL, "code"),
    d: s("code", "D", RED, "code"),
    nim: s("code", "NIM", YELLOW, "code"),
    zig: s("code", "ZIG", AMBER, "code"),
    v: s("code", "V", INDIGO, "code"),
    vala: s("code", "VALA", PURPLE, "code"),
    pas: s("code", "PAS", BLUE, "code"),
    cob: s("code", "COB", BLUE, "code"),
    f: s("code", "F", PURPLE, "code"),
    f90: s("code", "F90", PURPLE, "code"),
    cu: s("code", "CU", GREEN, "code"),
    asm: s("code", "ASM", GRAPHITE, "code"),
    s: s("code", "S", GRAPHITE, "code"),
    wasm: s("code", "WASM", PURPLE, "code"),

    // ── Dynamic & scripting languages ────────────────────────────────────
    rb: s("code", "RB", RED, "code"),
    erb: s("code", "ERB", RED, "code"),
    php: s("code", "PHP", INDIGO, "code"),
    lua: s("code", "LUA", VIOLET, "code"),
    pl: s("code", "PL", BROWN, "code"),
    pm: s("code", "PM", BROWN, "code"),

    // ── Functional languages ─────────────────────────────────────────────
    ex: s("code", "EX", PURPLE, "code"),
    exs: s("code", "EXS", PURPLE, "code"),
    erl: s("code", "ERL", RED, "code"),
    hs: s("code", "HS", VIOLET, "code"),
    elm: s("code", "ELM", SKY, "code"),
    clj: s("code", "CLJ", GREEN, "code"),
    cljs: s("code", "CLJS", GREEN, "code"),
    cljc: s("code", "CLJC", GREEN, "code"),
    ml: s("code", "ML", ORANGE, "code"),
    mli: s("code", "MLI", ORANGE, "code"),
    res: s("code", "RES", RED, "code"),
    rei: s("code", "REI", RED, "code"),

    // ── Shells ───────────────────────────────────────────────────────────
    sh: s("executable", "SH", GREEN, "code"),
    bash: s("executable", "BASH", GREEN, "code"),
    zsh: s("executable", "ZSH", GREEN, "code"),
    fish: s("executable", "FISH", MINT, "code"),
    ps1: s("executable", "PS1", BLUE, "code"),
    bat: s("executable", "BAT", SLATE, "code"),
    cmd: s("executable", "CMD", SLATE, "code"),

    // ── Queries, schemas & shaders ───────────────────────────────────────
    sql: s("database", "SQL", SKY, "code"),
    psql: s("database", "PSQL", SKY, "code"),
    prisma: s("database", "PRSM", TEAL, "code"),
    graphql: s("code", "GQL", PINK, "code"),
    gql: s("code", "GQL", PINK, "code"),
    proto: s("code", "PROT", INDIGO, "code"),
    sol: s("code", "SOL", GRAPHITE, "code"),
    glsl: s("code", "GLSL", PINK, "code"),
    hlsl: s("code", "HLSL", PINK, "code"),
    wgsl: s("code", "WGSL", PINK, "code"),

    // ── Component frameworks ─────────────────────────────────────────────
    vue: s("code", "VUE", GREEN, "code"),
    svelte: s("code", "SVLT", ORANGE, "code"),
    astro: s("code", "ASTR", PURPLE, "code"),

    // ── Web documents & styling ──────────────────────────────────────────
    html: s("web", "HTML", ORANGE, "code"),
    htm: s("web", "HTM", ORANGE, "code"),
    xhtml: s("web", "XHTM", ORANGE, "code"),
    css: s("web", "CSS", BLUE, "code"),
    scss: s("web", "SCSS", PINK, "code"),
    sass: s("web", "SASS", PINK, "code"),
    less: s("web", "LESS", INDIGO, "code"),
    styl: s("web", "STYL", GREEN, "code"),
    pcss: s("web", "PCSS", RED, "code"),
    map: s("data", "MAP", SLATE, "code"),
    webmanifest: s("config", "WMAN", SKY, "code"),
    htaccess: s("config", "HTAC", GRAPHITE, "code"),

    // ── Structured configuration ─────────────────────────────────────────
    json: s("data", "JSON", AMBER, "code"),
    jsonc: s("data", "JSNC", AMBER, "code"),
    json5: s("data", "JSN5", AMBER, "code"),
    yaml: s("data", "YAML", RED, "code"),
    yml: s("data", "YML", RED, "code"),
    toml: s("config", "TOML", BROWN, "code"),
    xml: s("data", "XML", ORANGE, "code"),
    plist: s("config", "PLST", SLATE, "code"),
    ini: s("config", "INI", SLATE, "code"),
    cfg: s("config", "CFG", SLATE, "code"),
    conf: s("config", "CONF", SLATE, "code"),
    properties: s("config", "PROP", SLATE, "code"),
    env: s("config", "ENV", YELLOW, "code"),
    "env.local": s("config", "ENVL", YELLOW, "code"),
    "env.example": s("config", "ENVX", YELLOW, "code"),
    lock: s("config", "LOCK", AMBER, "code"),

    // ── Toolchain dotfiles & build files ─────────────────────────────────
    npmrc: s("config", "NPM", RED, "code"),
    nvmrc: s("config", "NVM", GREEN, "code"),
    editorconfig: s("config", "EDTR", SLATE, "code"),
    gitignore: s("config", "GIT", ORANGE, "code"),
    gitattributes: s("config", "GITA", ORANGE, "code"),
    dockerignore: s("config", "DIGN", SKY, "code"),
    eslintrc: s("config", "ESLT", VIOLET, "code"),
    prettierrc: s("config", "PRTR", MINT, "code"),
    babelrc: s("config", "BABL", YELLOW, "code"),
    makefile: s("config", "MAKE", RUST, "code"),
    dockerfile: s("config", "DOCK", SKY, "code"),
    cmake: s("config", "CMAK", GREEN, "code"),
    gradle: s("config", "GRDL", TEAL, "code"),
    pom: s("config", "POM", RED, "code"),
    xcconfig: s("config", "XCFG", BLUE, "code"),
    pbxproj: s("config", "PBX", BLUE, "code"),
    storyboard: s("config", "STRY", BLUE, "code"),
    xib: s("config", "XIB", BLUE, "code"),

    // ── Scientific & tabular data ────────────────────────────────────────
    avro: s("data", "AVRO", SKY, "data"),
    parquet: s("data", "PARQ", CYAN, "data"),
    feather: s("data", "FTHR", TEAL, "data"),
    arrow: s("data", "ARRW", TEAL, "data"),
    h5: s("data", "H5", PURPLE, "data"),
    hdf5: s("data", "HDF5", PURPLE, "data"),
    npy: s("data", "NPY", INDIGO, "data"),
    npz: s("data", "NPZ", INDIGO, "data"),
    pkl: s("data", "PKL", BROWN, "data"),
    mat: s("data", "MAT", ORANGE, "data"),
    rds: s("data", "RDS", BLUE, "data"),
    sav: s("data", "SAV", VIOLET, "data"),
    dta: s("data", "DTA", RED, "data"),
    ndjson: s("data", "NDJS", AMBER, "data"),

    // ── Geo, calendar & contacts ─────────────────────────────────────────
    geojson: s("data", "GEO", GREEN, "data"),
    kml: s("data", "KML", GREEN, "data"),
    gpx: s("data", "GPX", LIME, "data"),
    ics: s("data", "ICS", SKY, "data"),
    vcf: s("data", "VCF", PINK, "data"),

    // ── Raster images ────────────────────────────────────────────────────
    png: s("image", "PNG", SKY, "image"),
    jpg: s("image", "JPG", AMBER, "image"),
    jpeg: s("image", "JPEG", AMBER, "image"),
    gif: s("image", "GIF", PINK, "image"),
    webp: s("image", "WEBP", TEAL, "image"),
    avif: s("image", "AVIF", MINT, "image"),
    heic: s("image", "HEIC", PURPLE, "image"),
    heif: s("image", "HEIF", PURPLE, "image"),
    bmp: s("image", "BMP", SLATE, "image"),
    tif: s("image", "TIF", INDIGO, "image"),
    tiff: s("image", "TIFF", INDIGO, "image"),
    ico: s("image", "ICO", CYAN, "image"),
    icns: s("image", "ICNS", CYAN, "image"),

    // ── Camera raw ───────────────────────────────────────────────────────
    raw: s("image", "RAW", GRAPHITE, "image"),
    dng: s("image", "DNG", GRAPHITE, "image"),
    cr2: s("image", "CR2", BROWN, "image"),
    cr3: s("image", "CR3", BROWN, "image"),
    nef: s("image", "NEF", YELLOW, "image"),
    arw: s("image", "ARW", ORANGE, "image"),
    orf: s("image", "ORF", RED, "image"),
    rw2: s("image", "RW2", BLUE, "image"),
    raf: s("image", "RAF", GREEN, "image"),

    // ── High dynamic range & texture formats ─────────────────────────────
    exr: s("image", "EXR", VIOLET, "image"),
    hdr: s("image", "HDR", VIOLET, "image"),
    tga: s("image", "TGA", SLATE, "image"),
    dds: s("image", "DDS", GRAPHITE, "image"),
    ktx: s("image", "KTX", GRAPHITE, "image"),
    ktx2: s("image", "KTX2", GRAPHITE, "image"),

    // ── Vector artwork ───────────────────────────────────────────────────
    svg: s("vector", "SVG", ORANGE, "image"),
    svgz: s("vector", "SVGZ", ORANGE, "image"),
    ai: s("vector", "AI", AMBER, "design"),
    eps: s("vector", "EPS", AMBER, "design"),

    // ── Design documents ─────────────────────────────────────────────────
    psd: s("design", "PSD", BLUE, "design"),
    psb: s("design", "PSB", BLUE, "design"),
    fig: s("design", "FIG", VIOLET, "design"),
    sketch: s("design", "SKCH", YELLOW, "design"),
    xd: s("design", "XD", PINK, "design"),
    indd: s("design", "INDD", PINK, "design"),
    idml: s("design", "IDML", PINK, "design"),
    ase: s("design", "ASE", TEAL, "design"),
    aseprite: s("design", "ASPR", RED, "design"),
    afdesign: s("design", "AFD", BLUE, "design"),
    afphoto: s("design", "AFP", PURPLE, "design"),
    afpub: s("design", "AFPB", TEAL, "design"),
    framer: s("design", "FRMR", SKY, "design"),
    pen: s("design", "PEN", GRAPHITE, "design"),
    lottie: s("design", "LOTT", MINT, "design"),

    // ── Video containers ─────────────────────────────────────────────────
    mp4: s("video", "MP4", SKY, "video"),
    m4v: s("video", "M4V", SKY, "video"),
    mov: s("video", "MOV", PURPLE, "video"),
    avi: s("video", "AVI", INDIGO, "video"),
    mkv: s("video", "MKV", GREEN, "video"),
    webm: s("video", "WEBM", TEAL, "video"),
    wmv: s("video", "WMV", BLUE, "video"),
    flv: s("video", "FLV", RED, "video"),
    mpg: s("video", "MPG", AMBER, "video"),
    mpeg: s("video", "MPEG", AMBER, "video"),
    mpeg2: s("video", "MPG2", AMBER, "video"),
    mts: s("video", "MTS", CYAN, "video"),
    m2ts: s("video", "M2TS", CYAN, "video"),
    "3gp": s("video", "3GP", SLATE, "video"),
    ogv: s("video", "OGV", LIME, "video"),
    vob: s("video", "VOB", GRAPHITE, "video"),
    mxf: s("video", "MXF", BROWN, "video"),

    // ── Camera & mastering codecs ────────────────────────────────────────
    prores: s("video", "PRO", VIOLET, "video"),
    braw: s("video", "BRAW", ORANGE, "video"),
    r3d: s("video", "R3D", RED, "video"),

    // ── Edit projects ────────────────────────────────────────────────────
    aep: s("video", "AEP", PURPLE, "video"),
    prproj: s("video", "PRPJ", VIOLET, "video"),
    drp: s("video", "DRP", ORANGE, "video"),
    fcpxml: s("video", "FCPX", PINK, "video"),
    fcpbundle: s("video", "FCPB", PINK, "video"),

    // ── Subtitles ────────────────────────────────────────────────────────
    srt: s("text", "SRT", SLATE, "video"),
    vtt: s("text", "VTT", SLATE, "video"),
    ass: s("text", "ASS", SLATE, "video"),
    sub: s("text", "SUB", SLATE, "video"),

    // ── Audio ────────────────────────────────────────────────────────────
    mp3: s("audio", "MP3", PINK, "audio"),
    wav: s("audio", "WAV", CYAN, "audio"),
    aiff: s("audio", "AIFF", CYAN, "audio"),
    aif: s("audio", "AIF", CYAN, "audio"),
    flac: s("audio", "FLAC", TEAL, "audio"),
    ogg: s("audio", "OGG", LIME, "audio"),
    oga: s("audio", "OGA", LIME, "audio"),
    opus: s("audio", "OPUS", MINT, "audio"),
    m4a: s("audio", "M4A", PURPLE, "audio"),
    aac: s("audio", "AAC", PURPLE, "audio"),
    wma: s("audio", "WMA", BLUE, "audio"),
    alac: s("audio", "ALAC", TEAL, "audio"),
    mid: s("audio", "MID", AMBER, "audio"),
    midi: s("audio", "MIDI", AMBER, "audio"),
    caf: s("audio", "CAF", SKY, "audio"),
    amr: s("audio", "AMR", SLATE, "audio"),
    ape: s("audio", "APE", GRAPHITE, "audio"),

    // ── Session files from the DAWs ──────────────────────────────────────
    logicx: s("audio", "LOGC", VIOLET, "audio"),
    als: s("audio", "ALS", YELLOW, "audio"),
    flp: s("audio", "FLP", ORANGE, "audio"),
    ptx: s("audio", "PTX", GREEN, "audio"),
    band: s("audio", "BAND", RED, "audio"),

    // ── 3D interchange ───────────────────────────────────────────────────
    obj: s("model3d", "OBJ", SLATE, "other"),
    fbx: s("model3d", "FBX", INDIGO, "other"),
    gltf: s("model3d", "GLTF", ORANGE, "other"),
    glb: s("model3d", "GLB", ORANGE, "other"),
    stl: s("model3d", "STL", CYAN, "other"),
    usd: s("model3d", "USD", PURPLE, "other"),
    usdz: s("model3d", "USDZ", PURPLE, "other"),
    usda: s("model3d", "USDA", PURPLE, "other"),
    usdc: s("model3d", "USDC", PURPLE, "other"),
    "3ds": s("model3d", "3DS", TEAL, "other"),
    dae: s("model3d", "DAE", GREEN, "other"),
    ply: s("model3d", "PLY", MINT, "other"),
    abc: s("model3d", "ABC", BROWN, "other"),

    // ── 3D authoring & CAD ───────────────────────────────────────────────
    blend: s("model3d", "BLND", ORANGE, "other"),
    c4d: s("model3d", "C4D", RED, "other"),
    ma: s("model3d", "MA", TEAL, "other"),
    mb: s("model3d", "MB", TEAL, "other"),
    max: s("model3d", "MAX", GRAPHITE, "other"),
    skp: s("model3d", "SKP", RED, "other"),
    step: s("model3d", "STEP", BLUE, "other"),
    stp: s("model3d", "STP", BLUE, "other"),
    iges: s("model3d", "IGES", SKY, "other"),
    igs: s("model3d", "IGS", SKY, "other"),

    // ── Archives ─────────────────────────────────────────────────────────
    zip: s("archive", "ZIP", AMBER, "archive"),
    rar: s("archive", "RAR", VIOLET, "archive"),
    "7z": s("archive", "7Z", SLATE, "archive"),
    tar: s("archive", "TAR", BROWN, "archive"),
    gz: s("archive", "GZ", BROWN, "archive"),
    tgz: s("archive", "TGZ", BROWN, "archive"),
    "tar.gz": s("archive", "TGZ", BROWN, "archive"),
    bz2: s("archive", "BZ2", RUST, "archive"),
    "tar.bz2": s("archive", "TBZ2", RUST, "archive"),
    xz: s("archive", "XZ", GRAPHITE, "archive"),
    "tar.xz": s("archive", "TXZ", GRAPHITE, "archive"),
    zst: s("archive", "ZST", CYAN, "archive"),
    lz4: s("archive", "LZ4", CYAN, "archive"),
    lzma: s("archive", "LZMA", GRAPHITE, "archive"),
    cab: s("archive", "CAB", SLATE, "archive"),
    z: s("archive", "Z", GRAPHITE, "archive"),
    arj: s("archive", "ARJ", SLATE, "archive"),
    sit: s("archive", "SIT", AMBER, "archive"),
    sitx: s("archive", "SITX", AMBER, "archive"),

    // ── Packaged bundles ─────────────────────────────────────────────────
    jar: s("archive", "JAR", RED, "archive"),
    war: s("archive", "WAR", RED, "archive"),
    ear: s("archive", "EAR", RED, "archive"),
    aar: s("archive", "AAR", GREEN, "archive"),
    whl: s("archive", "WHL", PYTHON, "archive"),
    egg: s("archive", "EGG", PYTHON, "archive"),
    crate: s("archive", "CRAT", RUST, "archive"),
    nupkg: s("archive", "NPKG", PURPLE, "archive"),

    // ── Disk & volume images ─────────────────────────────────────────────
    dmg: s("disk", "DMG", SLATE, "archive"),
    iso: s("disk", "ISO", SKY, "archive"),
    img: s("disk", "IMG", SLATE, "archive"),
    vmdk: s("disk", "VMDK", BLUE, "archive"),
    vdi: s("disk", "VDI", BLUE, "archive"),
    vhd: s("disk", "VHD", INDIGO, "archive"),
    vhdx: s("disk", "VHDX", INDIGO, "archive"),
    qcow2: s("disk", "QCOW", TEAL, "archive"),
    bin: s("disk", "BIN", GRAPHITE, "archive"),
    cue: s("disk", "CUE", AMBER, "archive"),
    nrg: s("disk", "NRG", ORANGE, "archive"),
    toast: s("disk", "TOST", AMBER, "archive"),
    sparseimage: s("disk", "SPRS", CYAN, "archive"),

    // ── Applications & installers ────────────────────────────────────────
    exe: s("executable", "EXE", BLUE, "other"),
    msi: s("executable", "MSI", BLUE, "other"),
    app: s("executable", "APP", CYAN, "other"),
    pkg: s("executable", "PKG", AMBER, "other"),
    deb: s("executable", "DEB", RED, "other"),
    rpm: s("executable", "RPM", RED, "other"),
    apk: s("executable", "APK", GREEN, "other"),
    ipa: s("executable", "IPA", SLATE, "other"),
    appimage: s("executable", "APPI", ORANGE, "other"),
    snap: s("executable", "SNAP", ORANGE, "other"),
    flatpak: s("executable", "FLAT", INDIGO, "other"),
    run: s("executable", "RUN", GREEN, "other"),
    com: s("executable", "COM", GRAPHITE, "other"),

    // ── Libraries & build output ─────────────────────────────────────────
    dll: s("executable", "DLL", SLATE, "other"),
    so: s("executable", "SO", SLATE, "other"),
    dylib: s("executable", "DYLB", SLATE, "other"),
    lib: s("executable", "LIB", GRAPHITE, "other"),
    a: s("executable", "A", GRAPHITE, "other"),
    o: s("executable", "O", GRAPHITE, "other"),
    out: s("executable", "OUT", GRAPHITE, "other"),

    // ── Databases ────────────────────────────────────────────────────────
    db: s("database", "DB", SKY, "data"),
    sqlite: s("database", "SQLT", SKY, "data"),
    sqlite3: s("database", "SQL3", SKY, "data"),
    mdb: s("database", "MDB", RED, "data"),
    accdb: s("database", "ACDB", RED, "data"),
    dbf: s("database", "DBF", BROWN, "data"),
    frm: s("database", "FRM", AMBER, "data"),
    ibd: s("database", "IBD", AMBER, "data"),
    realm: s("database", "RLM", PURPLE, "data"),

    // ── Certificates, keys & signatures ──────────────────────────────────
    pem: s("certificate", "PEM", GREEN, "other"),
    crt: s("certificate", "CRT", GREEN, "other"),
    cer: s("certificate", "CER", GREEN, "other"),
    der: s("certificate", "DER", TEAL, "other"),
    p12: s("certificate", "P12", AMBER, "other"),
    pfx: s("certificate", "PFX", AMBER, "other"),
    csr: s("certificate", "CSR", MINT, "other"),
    gpg: s("certificate", "GPG", VIOLET, "other"),
    pgp: s("certificate", "PGP", VIOLET, "other"),
    asc: s("certificate", "ASC", PURPLE, "other"),
    sig: s("certificate", "SIG", CYAN, "other"),
    license: s("certificate", "LIC", SLATE, "other"),

    // ── Fonts ────────────────────────────────────────────────────────────
    ttf: s("font", "TTF", PURPLE, "other"),
    otf: s("font", "OTF", VIOLET, "other"),
    woff: s("font", "WOFF", PINK, "other"),
    woff2: s("font", "WOF2", PINK, "other"),
    eot: s("font", "EOT", SLATE, "other"),
    fon: s("font", "FON", GRAPHITE, "other"),
    pfb: s("font", "PFB", BROWN, "other"),
    pfa: s("font", "PFA", BROWN, "other"),
    ufo: s("font", "UFO", TEAL, "other"),

    // ── Color science ───────────────────────────────────────────────────
    cube: s("data", "CUBE", VIOLET, "other"),
    lut: s("data", "LUT", VIOLET, "other"),
    icc: s("data", "ICC", PINK, "other"),
    icm: s("data", "ICM", PINK, "other"),

    // ── Shortcuts, transfers & leftovers ─────────────────────────────────
    torrent: s("data", "TORR", GREEN, "other"),
    url: s("web", "URL", SKY, "other"),
    webloc: s("web", "WLOC", SKY, "other"),
    lnk: s("generic", "LNK", SLATE, "other"),
    desktop: s("config", "DESK", SLATE, "other"),
    bak: s("generic", "BAK", GRAPHITE, "other"),
    tmp: s("generic", "TMP", GRAPHITE, "other"),
    temp: s("generic", "TEMP", GRAPHITE, "other"),
    swp: s("generic", "SWP", GRAPHITE, "other"),
    part: s("generic", "PART", SLATE, "other"),
    crdownload: s("generic", "CRDL", SLATE, "other"),
  } satisfies Record<string, IconSpec>,
);

/**
 * Whole file names that carry a type without an extension.
 *
 * `Makefile`, `Dockerfile` and `LICENSE` are as recognisable as any extension,
 * and `extOf` returns "" for all three — without this table the most
 * characteristic files in a repository would be the only gray blanks in the
 * grid. Keys are lowercased whole base names, so lookup is case-insensitive
 * and `.gitignore` keeps its leading dot.
 */
export const NAME_SPECS: Record<string, IconSpec> = Object.assign(
  Object.create(null) as Record<string, IconSpec>,
  {
    makefile: s("config", "MAKE", RUST, "code"),
    gnumakefile: s("config", "MAKE", RUST, "code"),
    dockerfile: s("config", "DOCK", SKY, "code"),
    containerfile: s("config", "DOCK", SKY, "code"),
    rakefile: s("config", "RAKE", RED, "code"),
    gemfile: s("config", "GEM", RED, "code"),
    procfile: s("config", "PROC", VIOLET, "code"),
    justfile: s("config", "JUST", AMBER, "code"),
    brewfile: s("config", "BREW", AMBER, "code"),
    vagrantfile: s("config", "VGRT", CYAN, "code"),
    license: s("certificate", "LIC", SLATE, "other"),
    licence: s("certificate", "LIC", SLATE, "other"),
    notice: s("certificate", "NOTE", SLATE, "other"),
    readme: s("text", "READ", SKY, "document"),
    changelog: s("text", "CHNG", MINT, "document"),
    contributing: s("text", "CONT", MINT, "document"),
    authors: s("text", "AUTH", SLATE, "document"),
    codeowners: s("config", "OWNR", ORANGE, "code"),
    ".gitignore": s("config", "GIT", ORANGE, "code"),
    ".gitattributes": s("config", "GITA", ORANGE, "code"),
    ".env": s("config", "ENV", YELLOW, "code"),
    ".editorconfig": s("config", "EDTR", SLATE, "code"),
    ".dockerignore": s("config", "DIGN", SKY, "code"),
  } satisfies Record<string, IconSpec>,
);

/** How many extensions the registry knows — what the icon gallery counts. */
export const EXTENSION_COUNT = Object.keys(EXTENSION_SPECS).length;

/**
 * Two-part extensions that must beat the last dot.
 *
 * `bundle.min.js` is minified JavaScript and `schema.d.ts` is a declaration
 * file; taking only the final segment would flatten both into their base
 * language and lose the distinction the icon exists to show. Longest first, so
 * `.tar.gz` never resolves as `.gz`.
 */
const COMPOUND_EXTENSIONS: readonly string[] = [
  "tar.bz2",
  "env.example",
  "tar.gz",
  "tar.xz",
  "env.local",
  "spec.ts",
  "test.ts",
  "min.js",
  "d.ts",
];

/**
 * The lowercase extension of a file name, without the dot.
 *
 * Dotfiles are named by their whole name (`.env` → `env`, `.gitignore` →
 * `gitignore`): the leading dot is the Unix "hidden" marker, not a type, and
 * `.env` is a recognisable *kind* of file the registry should be allowed to
 * draw. A dotfile that carries a real extension (`.eslintrc.json`) keeps it.
 * Returns "" when there is no extension at all.
 */
export function extOf(name: string): string {
  const base = name.split("/").pop()?.split("\\").pop() ?? "";
  const lower = base.toLowerCase().trim();
  if (lower === "") return "";

  const stripped = lower.replace(/^\.+/, "");
  if (stripped === "") return "";
  // A dotfile with no further dot is its own type: ".env" → "env".
  if (stripped.length !== lower.length && !stripped.includes(".")) return stripped;

  // `=== compound` catches the dotfile forms: ".env.local" strips to
  // "env.local", which has no preceding dot to anchor the suffix test.
  for (const compound of COMPOUND_EXTENSIONS) {
    if (stripped === compound || stripped.endsWith(`.${compound}`)) return compound;
  }

  const dot = stripped.lastIndexOf(".");
  if (dot <= 0 || dot === stripped.length - 1) return "";
  return stripped.slice(dot + 1);
}

/** The lowercased final path segment, which is what `NAME_SPECS` is keyed by. */
function baseNameOf(name: string): string {
  return (name.split("/").pop()?.split("\\").pop() ?? "").toLowerCase().trim();
}

/**
 * The drawing for a file name; `GENERIC_SPEC` when nothing matches.
 *
 * Resolution runs most-specific first: the compound extension (`spec.ts`), then
 * its tail (`ts`), then the whole file name (`Makefile`). The tail step is the
 * one that is easy to miss — `extOf` must keep returning `spec.ts` so a row for
 * it can exist, which would otherwise make every untabled compound
 * (`Button.test.tsx`, `App.stories.ts`) fall all the way to gray even though
 * the language is right there in the name.
 *
 * The own-property checks belt the null-prototype braces: the lookup key is
 * attacker-shaped data (any name on disk), and a future edit that reverts a
 * table to a plain literal must not silently start returning
 * `Object.prototype` members to the renderer.
 */
export function iconSpecForName(name: string): IconSpec {
  const ext = extOf(name);
  if (ext !== "" && Object.hasOwn(EXTENSION_SPECS, ext)) return EXTENSION_SPECS[ext];

  const dot = ext.lastIndexOf(".");
  if (dot > 0) {
    const tail = ext.slice(dot + 1);
    if (Object.hasOwn(EXTENSION_SPECS, tail)) return EXTENSION_SPECS[tail];
  }

  const base = baseNameOf(name);
  if (Object.hasOwn(NAME_SPECS, base)) return NAME_SPECS[base];

  return GENERIC_SPEC;
}

/** The coarse bucket a file name falls in — what filtering and grouping use. */
export function iconCategoryForName(name: string): IconCategory {
  return iconSpecForName(name).category;
}

/**
 * Every row, ordered the way the icon gallery shows them: grouped by family in
 * drawing-registry order, alphabetical inside each group. Sorting here rather
 * than in the gallery keeps the one canonical ordering next to the data it
 * orders, so a new family lands in the right place without touching the view.
 */
export function allIconSpecs(): { ext: string; spec: IconSpec }[] {
  const rank = new Map<IconFamily, number>(ICON_FAMILIES.map((f, i) => [f, i]));
  return Object.keys(EXTENSION_SPECS)
    .map((ext) => ({ ext, spec: EXTENSION_SPECS[ext] }))
    .sort((a, b) => {
      const fa = rank.get(a.spec.family) ?? ICON_FAMILIES.length;
      const fb = rank.get(b.spec.family) ?? ICON_FAMILIES.length;
      return fa !== fb ? fa - fb : a.ext.localeCompare(b.ext);
    });
}
