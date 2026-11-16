//! What jport's optimized mode (`JPORT_OPT=1`) writes calls to: Java's arithmetic on plain Rust numbers, and calls
//! into the plugin's own methods that skip the lookup by name. Every helper keeps Java's exact meaning: integer
//! division by zero throws `ArithmeticException`, `Integer.MIN_VALUE / -1` wraps, a call on `null` throws
//! `NullPointerException`.
use crate::{exc, guarded, Method, R, V};

/// `a / b` on two `int`s.
#[inline]
pub fn idiv(a: i32, b: i32) -> Result<i32, V> {
    if b == 0 { Err(exc("ArithmeticException", "/ by zero")) } else { Ok(a.wrapping_div(b)) }
}

/// `a % b` on two `int`s.
#[inline]
pub fn irem(a: i32, b: i32) -> Result<i32, V> {
    if b == 0 { Err(exc("ArithmeticException", "/ by zero")) } else { Ok(a.wrapping_rem(b)) }
}

/// `a / b` on two `long`s.
#[inline]
pub fn ldiv(a: i64, b: i64) -> Result<i64, V> {
    if b == 0 { Err(exc("ArithmeticException", "/ by zero")) } else { Ok(a.wrapping_div(b)) }
}

/// `a % b` on two `long`s.
#[inline]
pub fn lrem(a: i64, b: i64) -> Result<i64, V> {
    if b == 0 { Err(exc("ArithmeticException", "/ by zero")) } else { Ok(a.wrapping_rem(b)) }
}

/// Java's `(int) d`: NaN is 0, out of range saturates (Rust's `as` does the same).
#[inline]
pub fn d2i(d: f64) -> i32 {
    d as i32
}

/// Java's `(long) d`.
#[inline]
pub fn d2l(d: f64) -> i64 {
    d as i64
}

fn null_call(name: &str) -> V {
    exc("NullPointerException", format!("Cannot invoke \"{name}()\" because the value is null"))
}

/// A call to one of the plugin's own methods that jport proved is the only one the call can reach (nothing in the
/// plugin overrides it, and no overload of the same arity competes): straight to it, through the same guard as a
/// call by name (stack depth, stack trace, a growing stack), without finding it by name first.
#[inline]
pub fn direct(this: &V, owner: &'static str, name: &'static str, method: Method, args: &[V]) -> R {
    if this.is_null() {
        return Err(null_call(name));
    }
    guarded(owner, name, || method(this, args))
}

/// `direct` for a method that calls nothing (a getter, a small computation): it can't recurse or run deep, so it
/// needs no guard at all.
#[inline]
pub fn leaf(this: &V, name: &'static str, method: Method, args: &[V]) -> R {
    if this.is_null() {
        return Err(null_call(name));
    }
    method(this, args)
}

/// A static method of the plugin's, found as `direct` finds an instance one. `class` is the class's handle, asked for
/// after the arguments are worked out: that runs its static initialiser first, at the moment Java would.
#[inline]
pub fn direct_static(name: &'static str, method: Method, args: &[V], class: &'static crate::Class) -> R {
    guarded(class.name, name, || method(&V::Null, args))
}

/// `array.length`: an array, or a list the game handed over as one; `NullPointerException` on null.
#[inline]
pub fn length(array: &V) -> Result<i32, V> {
    match array {
        V::Arr(items) | V::List(items) => Ok(crate::lock(items).len() as i32),
        V::Null => Err(exc("NullPointerException", "Cannot read the array length because the value is null")),
        other => other.get("length").map(|v| v.int()),
    }
}

/// `array[i]` with a plain index: as `jrt::index`, bounds and null checked.
#[inline]
pub fn at(array: &V, index: i32) -> R {
    if let V::Arr(items) | V::List(items) = array {
        let items = crate::lock(items);
        return usize::try_from(index)
            .ok()
            .and_then(|i| items.get(i).cloned())
            .ok_or_else(|| exc("ArrayIndexOutOfBoundsException", format!("Index {index} out of bounds for length {}", items.len())));
    }
    crate::index(array, &V::Int(index))
}

/// A literal string or a game constant, made once: `cached(&SLOT, || jrt::s("text"))`. The value is an `Arc`, so each
/// use is a reference count, not a new allocation.
#[inline]
pub fn cached(slot: &'static std::sync::OnceLock<V>, make: impl FnOnce() -> V) -> V {
    slot.get_or_init(make).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_division() {
        assert_eq!(idiv(7, -2).ok(), Some(-3));
        assert_eq!(irem(-7, 2).ok(), Some(-1));
        assert_eq!(idiv(i32::MIN, -1).ok(), Some(i32::MIN));
        assert!(idiv(1, 0).is_err());
        assert_eq!(lrem(i64::MIN, -1).ok(), Some(0));
        assert_eq!(d2i(f64::NAN), 0);
        assert_eq!(d2i(1e20), i32::MAX);
        assert_eq!(d2l(-1e30), i64::MIN);
    }
}
