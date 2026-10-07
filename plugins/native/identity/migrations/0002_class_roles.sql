-- 0002：等级与角色的绑定（identity 03 §1.10 class_roles），由 levels 经 ctx.rbac.set_class_roles 维护。
CREATE TABLE class_roles (
    class_id      integer NOT NULL,
    role          text NOT NULL REFERENCES roles(name) ON DELETE CASCADE,
    owner_plugin  text NOT NULL,
    PRIMARY KEY (class_id, role)
);
