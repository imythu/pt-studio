-- 以超级用户执行一次（kernel/03 §6.1）。密码请替换。
CREATE ROLE pt_admin LOGIN CREATEROLE PASSWORD 'change-me-admin';
CREATE ROLE pt_gateway LOGIN NOINHERIT PASSWORD 'change-me-gateway';
-- pt_admin 需要能以 pt_gateway 身份建立 kernel schema 中的对象（属主为 pt_gateway）
GRANT pt_gateway TO pt_admin WITH INHERIT FALSE, SET TRUE;
CREATE DATABASE pt OWNER pt_admin;

-- 以下在目标数据库中执行（\c pt），需要超级用户：阻断经由函数的角色切换与 plpgsql（kernel/03 §6.1）
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
REVOKE EXECUTE ON FUNCTION pg_catalog.set_config(text, text, boolean) FROM PUBLIC;
GRANT  EXECUTE ON FUNCTION pg_catalog.set_config(text, text, boolean) TO pt_gateway, pt_admin;
REVOKE USAGE ON LANGUAGE plpgsql FROM PUBLIC;
GRANT  USAGE ON LANGUAGE plpgsql TO pt_gateway, pt_admin WITH GRANT OPTION;
