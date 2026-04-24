fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "none" || target_os == "uefi" {
        return;
    }

    build_sqlite();
    build_lua();
}

fn build_sqlite() {
    let sqlite_dir = "third_party/curated/sqlite";
    println!("cargo:rerun-if-changed={sqlite_dir}/sqlite3.c");
    println!("cargo:rerun-if-changed={sqlite_dir}/sqlite3.h");
    println!("cargo:rerun-if-changed={sqlite_dir}/sqlite3ext.h");

    let mut build = cc::Build::new();
    build
        .file(format!("{sqlite_dir}/sqlite3.c"))
        .include(sqlite_dir)
        .define("SQLITE_THREADSAFE", "0")
        .define("SQLITE_OMIT_LOAD_EXTENSION", "1")
        .define("SQLITE_ENABLE_API_ARMOR", "1")
        .warnings(false);

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.define("_CRT_SECURE_NO_WARNINGS", "1");
    }

    build.compile("echos_sqlite");
}

fn build_lua() {
    let lua_dir = "third_party/curated/lua/src";
    for file in [
        "lapi.c",
        "lauxlib.c",
        "lbaselib.c",
        "lcode.c",
        "lcorolib.c",
        "lctype.c",
        "ldebug.c",
        "ldo.c",
        "ldump.c",
        "lfunc.c",
        "lgc.c",
        "llex.c",
        "lmathlib.c",
        "lmem.c",
        "lobject.c",
        "lopcodes.c",
        "lparser.c",
        "lstate.c",
        "lstring.c",
        "lstrlib.c",
        "ltable.c",
        "ltablib.c",
        "ltm.c",
        "lundump.c",
        "lutf8lib.c",
        "lvm.c",
        "lzio.c",
    ] {
        println!("cargo:rerun-if-changed={lua_dir}/{file}");
    }
    println!("cargo:rerun-if-changed={lua_dir}/lua.h");
    println!("cargo:rerun-if-changed={lua_dir}/lauxlib.h");
    println!("cargo:rerun-if-changed={lua_dir}/lualib.h");
    println!("cargo:rerun-if-changed={lua_dir}/luaconf.h");

    let mut build = cc::Build::new();
    build
        .include(lua_dir)
        .define("LUA_USE_C89", "1")
        .warnings(false);

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.define("_CRT_SECURE_NO_WARNINGS", "1");
    }

    for file in [
        "lapi.c",
        "lauxlib.c",
        "lbaselib.c",
        "lcode.c",
        "lcorolib.c",
        "lctype.c",
        "ldebug.c",
        "ldo.c",
        "ldump.c",
        "lfunc.c",
        "lgc.c",
        "llex.c",
        "lmathlib.c",
        "lmem.c",
        "lobject.c",
        "lopcodes.c",
        "lparser.c",
        "lstate.c",
        "lstring.c",
        "lstrlib.c",
        "ltable.c",
        "ltablib.c",
        "ltm.c",
        "lundump.c",
        "lutf8lib.c",
        "lvm.c",
        "lzio.c",
    ] {
        build.file(format!("{lua_dir}/{file}"));
    }

    build.compile("echos_lua");
}
