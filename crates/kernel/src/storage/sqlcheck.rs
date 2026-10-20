//! 插件 SQL 语句检查（kernel/03 §6.3，FR-KRN-052）：WASM 插件的运行期语句与迁移文件。
//!
//! 背景：运行期连接的登录角色（网关）对所有插件角色都有 `SET` 权限，工作单元只是 `SET LOCAL ROLE <插件>`。
//! 插件 SQL 若能执行 `RESET ROLE`、`SET ROLE`、`set_config('role', …)`，或建一个带 `SET role = …` 子句 /
//! 函数体里 `SET ROLE` 的函数，就能切到网关或其他插件的角色，读写别的插件的数据；迁移在管理连接上执行，
//! 逃逸后甚至拿到管理角色。GRANT 隔离挡不住这些，所以对不受信任的 WASM 插件做词法检查：
//!
//! - 运行期：只允许单条 `SELECT` / `INSERT` / `UPDATE` / `DELETE` / `MERGE` / `WITH` / `VALUES` / `TABLE`，
//!   因而不能执行 `SET`、`RESET`、`DO`、DDL（不能在运行期建函数）；拒绝调用会切换角色、执行任意 SQL 文本或
//!   访问服务器资源的函数。
//! - 迁移：允许 DDL，但拒绝角色 / 会话切换（含函数的 `SET role` 子句）、`SECURITY DEFINER`、`sql` 以外的函数
//!   语言、`DO`、角色与扩展等对象；函数体（字符串 / `$$` 体）同样递归检查。
//!
//! 词法分析识别注释、字符串（含 `E''`、`$tag$`）与带引号的标识符，不会被注释或字符串中的文字误导；
//! `U&` 转义形式一律拒绝（可以拼出任意标识符）。这是纵深防御：数据库层另外撤销 `set_config` 的 PUBLIC 执行权
//! （deploy/postgres/bootstrap.sql）。

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    /// 未加引号的词（小写）
    Word(String),
    /// 带引号的标识符（原样）
    Quoted(String),
    /// 字符串字面量（内容；函数体在迁移检查中递归检查）
    Str(String),
    /// `$1` 形式的参数
    Param,
    Punct(char),
}

fn lex(sql: &str) -> Result<Vec<Tok>, String> {
    let cs: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let n = cs.len();
    while i < n {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '-' && cs.get(i + 1) == Some(&'-') {
            while i < n && cs[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && cs.get(i + 1) == Some(&'*') {
            // 块注释可嵌套
            let mut depth = 0usize;
            while i < n {
                if cs[i] == '/' && cs.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if cs[i] == '*' && cs.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            if depth != 0 {
                return Err("unterminated comment".into());
            }
        } else if c == '\'' {
            let (s, next) = quoted(&cs, i, '\'', false)?;
            out.push(Tok::Str(s));
            i = next;
        } else if c == '"' {
            let (s, next) = quoted(&cs, i, '"', false)?;
            out.push(Tok::Quoted(s));
            i = next;
        } else if c == '$' {
            if cs.get(i + 1).is_some_and(char::is_ascii_digit) {
                i += 1;
                while i < n && cs[i].is_ascii_digit() {
                    i += 1;
                }
                out.push(Tok::Param);
                continue;
            }
            // 美元引用：$tag$ … $tag$
            let mut j = i + 1;
            while j < n && (cs[j].is_alphanumeric() || cs[j] == '_') {
                j += 1;
            }
            if cs.get(j) != Some(&'$') {
                return Err("unexpected `$`".into());
            }
            let tag: String = cs[i..=j].iter().collect();
            let body_start = j + 1;
            let rest: String = cs[body_start..].iter().collect();
            let Some(end) = rest.find(&tag) else {
                return Err("unterminated dollar-quoted string".into());
            };
            out.push(Tok::Str(rest[..end].to_owned()));
            i = body_start + rest[..end].chars().count() + tag.chars().count();
        } else if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < n && (cs[j].is_alphanumeric() || cs[j] == '_' || cs[j] == '$') {
                j += 1;
            }
            let w: String = cs[i..j].iter().collect::<String>().to_lowercase();
            // U&'…' / U&"…"：Unicode 转义可以拼出任意标识符
            if w == "u" && cs.get(j) == Some(&'&') {
                return Err("U& escapes are not allowed".into());
            }
            // 前缀字符串：E'…'（反斜杠转义）、B'…'、X'…'、N'…'
            if cs.get(j) == Some(&'\'') && matches!(w.as_str(), "e" | "b" | "x" | "n") {
                let (s, next) = quoted(&cs, j, '\'', w == "e")?;
                out.push(Tok::Str(s));
                i = next;
                continue;
            }
            out.push(Tok::Word(w));
            i = j;
        } else if c.is_ascii_digit() {
            while i < n && (cs[i].is_ascii_alphanumeric() || cs[i] == '.') {
                i += 1;
            }
        } else {
            out.push(Tok::Punct(c));
            i += 1;
        }
    }
    Ok(out)
}

/// 读取引号内容（引号重复表示自身；`backslash` 时 `\x` 转义）
fn quoted(cs: &[char], start: usize, q: char, backslash: bool) -> Result<(String, usize), String> {
    let mut s = String::new();
    let mut i = start + 1;
    while i < cs.len() {
        let c = cs[i];
        if backslash && c == '\\' {
            if let Some(&nx) = cs.get(i + 1) {
                s.push(nx);
            }
            i += 2;
        } else if c == q {
            if cs.get(i + 1) == Some(&q) {
                s.push(q);
                i += 2;
            } else {
                return Ok((s, i + 1));
            }
        } else {
            s.push(c);
            i += 1;
        }
    }
    Err("unterminated quoted text".into())
}

/// 标识符的规范形式：未加引号的词已小写；带引号的按原样（与 PG 一致，`"SET_CONFIG"` 不是 set_config）
fn ident(t: &Tok) -> Option<&str> {
    match t {
        Tok::Word(w) | Tok::Quoted(w) => Some(w),
        _ => None,
    }
}

/// 不允许调用的函数（名称或前缀，后面紧跟 `(` 才算调用，避免误伤同名列）
const DENIED_FUNCS: &[&str] = &[
    "set_config",
    "query_to_xml",
    "query_to_xml_and_xmlschema",
    "query_to_xmlschema",
    "ts_stat",
    "ts_rewrite",
    "pg_read_file",
    "pg_read_binary_file",
    "pg_ls_dir",
    "pg_stat_file",
    "pg_terminate_backend",
    "pg_cancel_backend",
    "pg_reload_conf",
    "pg_rotate_logfile",
    "pg_notify",
];
const DENIED_FUNC_PREFIXES: &[&str] = &[
    "cursor_to_xml",
    "table_to_xml",
    "schema_to_xml",
    "database_to_xml",
    "dblink",
    "lo_",
    "pg_advisory",
    "pg_try_advisory",
    "pg_sleep",
    "pg_ls_",
];

fn denied_call(toks: &[Tok]) -> Option<String> {
    toks.windows(2).find_map(|w| {
        let name = ident(&w[0])?;
        if w[1] != Tok::Punct('(') {
            return None;
        }
        (DENIED_FUNCS.contains(&name) || DENIED_FUNC_PREFIXES.iter().any(|p| name.starts_with(p)))
            .then(|| format!("function `{name}` is not allowed"))
    })
}

const RUNTIME_FIRST: &[&str] = &[
    "select", "insert", "update", "delete", "merge", "with", "values", "table",
];

/// 运行期语句检查（WASM 插件的 `sql.query` / `sql.execute` / `sql.execute-batch`）
pub fn check_runtime(sql: &str) -> Result<(), String> {
    let toks = lex(sql)?;
    let first = toks.iter().find(|t| **t != Tok::Punct('('));
    match first.and_then(ident) {
        Some(w) if RUNTIME_FIRST.contains(&w) && matches!(first, Some(Tok::Word(_))) => {}
        Some(w) => return Err(format!("statement `{w}` is not allowed at runtime")),
        None => return Err("empty statement".into()),
    }
    // 只能有一条语句（末尾的分号可以有）
    if let Some(p) = toks.iter().position(|t| *t == Tok::Punct(';'))
        && toks[p..].iter().any(|t| *t != Tok::Punct(';'))
    {
        return Err("multiple statements are not allowed".into());
    }
    if let Some(e) = denied_call(&toks) {
        return Err(e);
    }
    Ok(())
}

/// 迁移文件检查（WASM 插件包）。函数体等字符串内容同样按 SQL 递归检查。
pub fn check_migration(sql: &str) -> Result<(), String> {
    check_migration_depth(sql, 0)
}

fn check_migration_depth(sql: &str, depth: u8) -> Result<(), String> {
    let toks = match lex(sql) {
        Ok(t) => t,
        // 字符串内容未必是 SQL（注释文字等）：内层词法失败时不作判断；最外层必须能解析
        Err(_) if depth > 0 => return Ok(()),
        Err(e) => return Err(e),
    };
    if let Some(e) = denied_call(&toks) {
        return Err(e);
    }
    let words: Vec<Option<&str>> = toks.iter().map(ident).collect();
    let at = |i: usize| words.get(i).copied().flatten();
    let mut stmt_start = true;
    for (i, t) in toks.iter().enumerate() {
        if *t == Tok::Punct(';') {
            stmt_start = true;
            continue;
        }
        if *t == Tok::Punct('(') {
            continue;
        }
        let w = at(i);
        if stmt_start {
            stmt_start = false;
            if let Some(w) = w
                && matches!(
                    w,
                    "set"
                        | "reset"
                        | "do"
                        | "call"
                        | "copy"
                        | "load"
                        | "listen"
                        | "notify"
                        | "begin"
                        | "start"
                        | "commit"
                        | "end"
                        | "rollback"
                        | "savepoint"
                        | "release"
                        | "prepare"
                        | "execute"
                        | "deallocate"
                        | "discard"
                        | "security"
                        | "import"
                        | "reassign"
                        | "checkpoint"
                )
            {
                return Err(format!("statement `{w}` is not allowed in plugin migrations"));
            }
        }
        match (w, at(i + 1), at(i + 2)) {
            // 角色 / 会话切换：SET [LOCAL|SESSION] ROLE、函数的 SET role 子句、SET SESSION AUTHORIZATION、RESET …
            (Some("set"), Some("role" | "session_authorization"), _)
            | (
                Some("set"),
                Some("local" | "session"),
                Some("role" | "authorization" | "session_authorization"),
            ) => {
                return Err("changing the role is not allowed".into());
            }
            (Some("reset"), Some("session"), Some("authorization"))
            | (Some("reset"), Some("role" | "all"), _) => {
                return Err("changing the role is not allowed".into());
            }
            (Some("security"), Some("definer"), _) => return Err("SECURITY DEFINER is not allowed".into()),
            (Some("language"), Some(lang), _) if lang != "sql" => {
                return Err(format!("function language `{lang}` is not allowed (only sql)"));
            }
            (Some("create" | "alter" | "drop"), Some(obj), _)
                if matches!(
                    obj,
                    "role"
                        | "user"
                        | "group"
                        | "extension"
                        | "schema"
                        | "database"
                        | "tablespace"
                        | "language"
                        | "event"
                        | "server"
                        | "foreign"
                        | "publication"
                        | "subscription"
                        | "system"
                        | "default"
                        | "policy"
                ) =>
            {
                return Err(format!(
                    "`{w} {obj}` is not allowed in plugin migrations",
                    w = w.unwrap_or("")
                ));
            }
            (Some("security_invoker"), ..) => return Err("security_invoker views are not allowed".into()),
            (Some("grant" | "revoke"), ..) => return Err("GRANT / REVOKE are managed by the kernel".into()),
            _ => {}
        }
    }
    // 函数体（`AS '…'` / `AS $$…$$`）里同样不能有上述内容；其他字符串（CHECK 取值、默认值等）不是 SQL
    if depth < 4 {
        for w in toks.windows(2) {
            if let (Tok::Word(kw), Tok::Str(body)) = (&w[0], &w[1])
                && kw == "as"
            {
                check_migration_depth(body, depth + 1)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {

    /// 仓库内所有 WASM 插件的迁移文件都应通过检查（预置数据不能用 `DO` 块等）
    #[test]
    fn repo_wasm_migrations_pass() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/wasm");
        let mut n = 0;
        for plugin in std::fs::read_dir(&root).into_iter().flatten().flatten() {
            for f in std::fs::read_dir(plugin.path().join("migrations")).into_iter().flatten().flatten() {
                let sql = std::fs::read_to_string(f.path()).unwrap_or_default();
                assert!(check_migration(&sql).is_ok(), "{}: {:?}", f.path().display(), check_migration(&sql));
                n += 1;
            }
        }
        assert!(n > 10);
    }

    use super::*;

    #[test]
    fn runtime_allows_dml() {
        for ok in [
            "SELECT id FROM classes WHERE active ORDER BY rank",
            "  (SELECT 1) UNION (SELECT 2)",
            "WITH x AS (DELETE FROM t RETURNING id) SELECT count(*) FROM x;",
            "INSERT INTO t (a, b) VALUES ($1, 'it''s; RESET ROLE') ON CONFLICT DO NOTHING",
            "UPDATE t SET reset_at = now(), role = $2 WHERE id = $1 -- SET ROLE x",
            "SELECT /* set_config( */ lo_count FROM t",
            "SELECT E'\\' ; set_config(' FROM t",
            "SELECT $tag$ ; RESET ROLE $tag$, \"set_config\" FROM t",
        ] {
            assert_eq!(check_runtime(ok), Ok(()), "{ok}");
        }
    }

    #[test]
    fn runtime_rejects_escapes() {
        for bad in [
            "RESET ROLE",
            "SET ROLE pt_plugin_identity",
            "set local role x",
            "SELECT set_config('role', 'x', true)",
            "SELECT pg_catalog.set_config ('role', 'x', true)",
            "SELECT \"set_config\"('role', 'x', true)",
            "SELECT 1; RESET ROLE",
            "DO $$ BEGIN PERFORM 1; END $$",
            "CREATE FUNCTION f() RETURNS text LANGUAGE sql SET role = 'x' AS 'SELECT 1'",
            "SELECT query_to_xml('select 1', true, false, '')",
            "SELECT ts_stat('select 1')",
            "SELECT U&\"\\0073et_config\"('role','x',true)",
            "SELECT pg_advisory_lock(1)",
            "COPY t TO '/tmp/x'",
            "",
            "SELECT 'unterminated",
        ] {
            assert!(check_runtime(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn migration_allows_ddl() {
        let sql = "-- levels 0001\nCREATE TABLE classes (id integer PRIMARY KEY, role text, reset_at timestamptz);\n\
                   CREATE INDEX classes_idx ON classes (id) WHERE role IS NOT NULL;\n\
                   ALTER TABLE classes ADD COLUMN name text NOT NULL DEFAULT '';\n\
                   CREATE VIEW v_x WITH (security_barrier = true) AS SELECT id FROM classes;\n\
                   CREATE FUNCTION f(a int) RETURNS int LANGUAGE sql IMMUTABLE AS $$ SELECT a + 1 $$;\n\
                   ALTER TABLE classes ADD CONSTRAINT k CHECK (role IN ('start', 'set role', 'reset'));\n\
                   INSERT INTO classes (id) VALUES (1);";
        assert_eq!(check_migration(sql), Ok(()));
    }

    #[test]
    fn migration_rejects_escapes() {
        for bad in [
            "CREATE TABLE t (id int); RESET ROLE; CREATE ROLE evil LOGIN SUPERUSER",
            "SET ROLE pt_admin",
            "CREATE FUNCTION f() RETURNS text LANGUAGE sql SET role = 'pt_plugin_identity' AS $$ SELECT 1 $$",
            "CREATE FUNCTION f() RETURNS text LANGUAGE sql AS $$ SET ROLE x; SELECT current_user::text $$",
            "CREATE FUNCTION f() RETURNS text LANGUAGE sql AS 'SELECT set_config(''role'', ''x'', true)'",
            "CREATE FUNCTION f() RETURNS void LANGUAGE plpgsql AS $$ BEGIN EXECUTE 'x'; END $$",
            "CREATE FUNCTION f() RETURNS int LANGUAGE sql SECURITY DEFINER AS $$ SELECT 1 $$",
            "DO $$ BEGIN END $$",
            "CREATE EXTENSION dblink",
            "CREATE SCHEMA other",
            "GRANT SELECT ON t TO PUBLIC",
            "ALTER FUNCTION f() SET role = 'x'",
            "CREATE VIEW v WITH (security_invoker = true) AS SELECT 1",
            "SET SESSION AUTHORIZATION postgres",
            "ALTER ROLE x SUPERUSER",
            "SELECT query_to_xml('SELECT 1', true, false, '')",
        ] {
            assert!(check_migration(bad).is_err(), "{bad}");
        }
    }

    /// 仓库内全部 WASM 插件的迁移都必须通过检查
    #[test]
    fn bundled_wasm_migrations_pass() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/wasm");
        let mut n = 0;
        for plugin in std::fs::read_dir(&root).unwrap().filter_map(Result::ok) {
            let Ok(files) = std::fs::read_dir(plugin.path().join("migrations")) else {
                continue;
            };
            for f in files.filter_map(Result::ok) {
                let sql = std::fs::read_to_string(f.path()).unwrap();
                assert_eq!(check_migration(&sql), Ok(()), "{}", f.path().display());
                n += 1;
            }
        }
        assert!(n > 20, "{n}");
    }
}
