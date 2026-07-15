import gdb
import sys

"""echOS GDB pretty-printers for Rust kernel types."""

def _get_variant_data(val, variant_name):
    """Try to extract data from a Rust enum variant."""
    try:
        variant = val[variant_name]
        return variant
    except Exception:
        return None

def _is_none(val):
    """Check if a Rust Option value is None."""
    none_variant = _get_variant_data(val, 'None')
    some_variant = _get_variant_data(val, 'Some')
    if some_variant is None:
        return True
    if none_variant is not None and some_variant is not None:
        try:
            disc = val['__discriminant']
            return int(disc) == 0
        except Exception:
            pass
        try:
            some_val = some_variant['__0']
            if some_val.type is not None:
                pass
        except Exception:
            return True
    return False

class OptionPrinter:
    """Pretty-printer for core::option::Option<T>."""

    def __init__(self, val):
        self.val = val

    def to_string(self):
        if _is_none(self.val):
            return "None"
        try:
            some_variant = self.val['Some']
            inner = some_variant['__0']
            return "Some({})".format(inner)
        except Exception:
            return "None"

    def children(self):
        if not _is_none(self.val):
            try:
                some_variant = self.val['Some']
                yield '__0', some_variant['__0']
            except Exception:
                pass

    def display_hint(self):
        return None


class ResultPrinter:
    """Pretty-printer for core::result::Result<T, E>."""

    def __init__(self, val):
        self.val = val

    def _is_ok(self):
        ok_variant = _get_variant_data(self.val, 'Ok')
        err_variant = _get_variant_data(self.val, 'Err')
        if ok_variant is None:
            return False
        if err_variant is not None:
            try:
                disc = self.val['__discriminant']
                return int(disc) == 0
            except Exception:
                pass
        return True

    def to_string(self):
        if self._is_ok():
            try:
                return "Ok({})".format(self.val['Ok']['__0'])
            except Exception:
                return "Ok"
        else:
            try:
                return "Err({})".format(self.val['Err']['__0'])
            except Exception:
                return "Err"

    def children(self):
        if self._is_ok():
            try:
                yield 'Ok', self.val['Ok']['__0']
            except Exception:
                pass
        else:
            try:
                yield 'Err', self.val['Err']['__0']
            except Exception:
                pass

    def display_hint(self):
        return None


class VecPrinter:
    """Pretty-printer for alloc::vec::Vec<T>."""

    def __init__(self, val):
        self.val = val
        self.len = int(val['len'])
        self.cap = int(val['buf']['cap'])
        try:
            self.ptr = val['buf']['ptr']['pointer']
        except Exception:
            self.ptr = val['buf']['ptr']

    def to_string(self):
        return "[{}]".format(", ".join(self._elements()))

    def _elements(self):
        result = []
        for i in range(self.len):
            try:
                elem = (self.ptr + i).dereference()
                result.append(str(elem))
            except Exception:
                result.append("?")
        return result

    def children(self):
        for i in range(self.len):
            try:
                yield (str(i), (self.ptr + i).dereference())
            except Exception:
                yield (str(i), "?")

    def display_hint(self):
        return "array"


class StringPrinter:
    """Pretty-printer for alloc::string::String."""

    def __init__(self, val):
        self.val = val
        self.vec = val['vec']

    def to_string(self):
        try:
            ptr = self.vec['buf']['ptr']['pointer']
            length = int(self.vec['len'])
            if length > 0:
                s = ptr.string(length=length)
                return '"{}"'.format(s)
            return '""'
        except Exception:
            return 'String({} bytes)'.format(self.vec['len'])

    def display_hint(self):
        return "string"


class MutexPrinter:
    """Pretty-printer for spin::mutex::Mutex<T>."""

    def __init__(self, val):
        self.val = val
        self._is_locked = False
        try:
            lock_field = val['lock']
            if lock_field is not None:
                val_str = str(lock_field)
                self._is_locked = 'true' in val_str.lower() or val_str != 'false'
        except Exception:
            pass
        try:
            locked_field = val['locked']
            if locked_field is not None:
                val_str = str(locked_field)
                self._is_locked = 'true' in val_str.lower() or val_str != 'false'
        except Exception:
            pass
        try:
            is_locked = val['is_locked']
            val_str = str(is_locked)
            self._is_locked = 'true' in val_str.lower() or val_str != 'false'
        except Exception:
            pass

    def to_string(self):
        return "Locked" if self._is_locked else "Unlocked"

    def children(self):
        if not self._is_locked:
            try:
                yield 'data', self.val['data']
            except Exception:
                pass

    def display_hint(self):
        return None


class BoxPrinter:
    """Pretty-printer for alloc::boxed::Box<T>."""

    def __init__(self, val):
        self.val = val

    def to_string(self):
        try:
            ptr = self.val['0']
            return "Box({})".format(ptr)
        except Exception:
            return "Box"

    def children(self):
        try:
            yield 'ptr', self.val['0']
        except Exception:
            pass

    def display_hint(self):
        return None


class ArcPrinter:
    """Pretty-printer for alloc::sync::Arc<T>."""

    def __init__(self, val):
        self.val = val

    def _get_counts(self):
        try:
            inner = self.val['inner']
            if inner.type is not None:
                data_ptr = inner
                inner_data = data_ptr.dereference()
                strong = int(inner_data['strong'])
                weak = int(inner_data['weak'])
                return strong, weak, inner_data['data']
        except Exception:
            pass
        try:
            ptr = self.val['ptr']
            inner_data = ptr.dereference()
            strong = int(inner_data['strong'])
            weak = int(inner_data['weak'])
            return strong, weak, inner_data['data']
        except Exception:
            pass
        try:
            data_val = self.val['data']
            strong = int(data_val['strong'])
            weak = int(data_val['weak'])
            return strong, weak, data_val['data']
        except Exception:
            pass
        return None, None, None

    def to_string(self):
        strong, weak, _ = self._get_counts()
        if strong is not None:
            return "Arc(strong={}, weak={})".format(strong, weak)
        return "Arc"

    def children(self):
        _, _, data = self._get_counts()
        if data is not None:
            yield 'data', data

    def display_hint(self):
        return None


class RcPrinter:
    """Pretty-printer for alloc::rc::Rc<T>."""

    def __init__(self, val):
        self.val = val

    def _get_counts(self):
        try:
            ptr = self.val['ptr']
            inner_data = ptr.dereference()
            strong = int(inner_data['strong'])
            weak = int(inner_data['weak'])
            return strong, weak, inner_data['data']
        except Exception:
            pass
        try:
            inner = self.val['inner']
            data_ptr = inner
            inner_data = data_ptr.dereference()
            strong = int(inner_data['strong'])
            weak = int(inner_data['weak'])
            return strong, weak, inner_data['data']
        except Exception:
            pass
        try:
            data_val = self.val['data']
            strong = int(data_val['strong'])
            weak = int(data_val['weak'])
            return strong, weak, data_val['data']
        except Exception:
            pass
        return None, None, None

    def to_string(self):
        strong, weak, _ = self._get_counts()
        if strong is not None:
            return "Rc(strong={}, weak={})".format(strong, weak)
        return "Rc"

    def children(self):
        _, _, data = self._get_counts()
        if data is not None:
            yield 'data', data

    def display_hint(self):
        return None


def _get_type_tag(val):
    """Extract the canonical type tag from a GDB value."""
    tag = val.type.tag
    if tag is not None:
        return tag
    try:
        t = val.type.strip_typedefs()
        if t is not None and t.tag is not None:
            return t.tag
    except Exception:
        pass
    return str(val.type)


def _type_tag_matches(tag, target):
    """Check if a type tag matches a target pattern (with generics stripped)."""
    if tag == target:
        return True
    if tag.startswith(target + '<') or tag.startswith(target + '__'):
        return True
    return False


def echos_pretty_lookup(val):
    """Lookup function for echOS pretty-printers."""
    tag = _get_type_tag(val)
    if tag is None:
        return None
    tag = tag.replace("'static ", "").replace("& ", "").replace("&mut ", "")
    if _type_tag_matches(tag, 'core::option::Option'):
        return OptionPrinter(val)
    if _type_tag_matches(tag, 'core::result::Result'):
        return ResultPrinter(val)
    if _type_tag_matches(tag, 'alloc::vec::Vec'):
        return VecPrinter(val)
    if _type_tag_matches(tag, 'alloc::string::String'):
        return StringPrinter(val)
    if _type_tag_matches(tag, 'spin::mutex::Mutex'):
        return MutexPrinter(val)
    if _type_tag_matches(tag, 'alloc::boxed::Box'):
        return BoxPrinter(val)
    if _type_tag_matches(tag, 'alloc::sync::Arc'):
        return ArcPrinter(val)
    if _type_tag_matches(tag, 'alloc::rc::Rc'):
        return RcPrinter(val)
    return None


def register_printers(objfile):
    """Register echOS pretty-printers on the given objfile."""
    objfile.pretty_printers.append(echos_pretty_lookup)
