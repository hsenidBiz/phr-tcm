def load(p): return open(p, encoding='utf-8', newline='').read()
def save(p, s): open(p, 'w', encoding='utf-8', newline='').write(s)

# ---- locate.rs: capture 0xfa string format + 0xee boolean per object ----
s = load('rpt-core/src/locate.rs')
nl = '\r\n' if '\r\n' in s[:3000] else '\n'
a = 'const T_COMMON_FIELD_FORMAT: u16 = 0x00f0;' + nl
assert a in s
s = s.replace(a, a + 'const T_STRING_FIELD_FORMAT: u16 = 0x00fa;' + nl + 'const T_BOOLEAN_FIELD_FORMAT: u16 = 0x00ee;' + nl, 1)
a = '    /// `0xf8` — numeric field formats (currency slot, then number slot).' + nl + '    pub numeric_formats: Vec<RecAddr>,'
assert a in s
s = s.replace(a, a + nl + '    /// `0xfa` — string field format (word wrap, max lines, interpretation, indents).' + nl + '    pub string_format: Option<RecAddr>,' + nl + '    /// `0xee` — boolean field format (output type).' + nl + '    pub boolean_format: Option<RecAddr>,', 1)
a = '            T_COMMON_FIELD_FORMAT => {' + nl
assert a in s
s = s.replace(a, '            T_STRING_FIELD_FORMAT => {' + nl + '                if let Some(obj) = self.obj_mut() {' + nl + '                    if obj.string_format.is_none() {' + nl + '                        obj.string_format = Some(addr);' + nl + '                    }' + nl + '                }' + nl + '            }' + nl + '            T_BOOLEAN_FIELD_FORMAT => {' + nl + '                if let Some(obj) = self.obj_mut() {' + nl + '                    if obj.boolean_format.is_none() {' + nl + '                        obj.boolean_format = Some(addr);' + nl + '                    }' + nl + '                }' + nl + '            }' + nl + a, 1)
save('rpt-core/src/locate.rs', s)
print('locate.rs ok')
