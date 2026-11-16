//! Java's collection views: `map.values()` and friends are the map, not a copy, and an iterator can remove
//! what it just gave. Also the rest of `List`, `Set`, `Map`, `Stream` and `Optional`.
use super::*;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Entries,
    Keys,
    Values,
}

pub struct JView {
    map: Arc<Mutex<JMap>>,
    kind: Kind,
}

pub struct JIterator {
    source: V,
    items: Vec<V>,
    at: Mutex<usize>,
    /// How many were removed from a list: positions after them move down.
    removed: Mutex<usize>,
}

type EntryOwners = std::collections::HashMap<usize, (std::sync::Weak<(V, V)>, std::sync::Weak<Mutex<JMap>>)>;

/// The map each entry of an `entrySet()` came from, while the entry lives: what `entry.setValue(x)` writes to.
static ENTRY_OWNERS: Mutex<Option<EntryOwners>> = Mutex::new(None);

fn remember_entry(pair: &Arc<(V, V)>, map: &Arc<Mutex<JMap>>) {
    let mut owners = lock(&ENTRY_OWNERS);
    let owners = owners.get_or_insert_default();
    if owners.len() >= 4096 {
        owners.retain(|_, (entry, _)| entry.strong_count() > 0);
    }
    owners.insert(Arc::as_ptr(pair) as usize, (Arc::downgrade(pair), Arc::downgrade(map)));
}

/// `Map.Entry.setValue(value)` on an entry of a map's `entrySet()`: the map gets the value, the old one comes back.
pub fn entry_set_value(pair: &Arc<(V, V)>, value: V) -> R {
    let owner = lock(&ENTRY_OWNERS).as_ref().and_then(|all| all.get(&(Arc::as_ptr(pair) as usize)).cloned());
    let map = owner.filter(|(entry, _)| entry.upgrade().is_some_and(|e| Arc::ptr_eq(&e, pair))).and_then(|(_, map)| map.upgrade());
    match map {
        Some(map) => {
            let old = lock(&map).insert(pair.0.key(), (pair.0.clone(), value)).map_or(V::Null, |(_, v)| v);
            Ok(old)
        }
        None => Err(exc("UnsupportedOperationException", "setValue on an entry no map backs")),
    }
}

fn snapshot(view: &JView) -> Vec<V> {
    let items: Vec<V> = lock(&view.map)
        .values()
        .map(|(k, v)| match view.kind {
            Kind::Entries => V::Entry(Arc::new((k.clone(), v.clone()))),
            Kind::Keys => k.clone(),
            Kind::Values => v.clone(),
        })
        .collect();
    if view.kind == Kind::Entries {
        for item in &items {
            if let V::Entry(pair) = item {
                remember_entry(pair, &view.map);
            }
        }
    }
    items
}

fn set_of(items: &[V]) -> V {
    V::Set(Arc::new(Mutex::new(items.iter().map(|k| (k.key(), (k.clone(), V::Null))).collect())))
}

/// As a plain collection, for everything that only reads.
fn plain(view: &JView) -> V {
    match view.kind {
        Kind::Values => list(snapshot(view)),
        _ => set_of(&snapshot(view)),
    }
}

fn drop_item(view: &JView, item: &V) -> bool {
    let mut map = lock(&view.map);
    match view.kind {
        Kind::Keys => map.remove(&item.key()).is_some(),
        Kind::Entries => match item {
            V::Entry(pair) => map.remove(&pair.0.key()).is_some(),
            _ => false,
        },
        Kind::Values => {
            let found = map.iter().find(|(_, (_, v))| same(v, item) || equals(v, item).is_ok_and(|e| e.truth())).map(|(k, _)| k.clone());
            found.is_some_and(|k| map.remove(&k).is_some())
        }
    }
}

/// `TreeSet` and `ConcurrentSkipListSet`: kept in the order of a comparator, which also says what's equal.
pub struct JSorted {
    items: Mutex<Vec<V>>,
    comparator: Option<V>,
}

impl JSorted {
    fn compare(&self, a: &V, b: &V) -> std::cmp::Ordering {
        match &self.comparator {
            Some(comparator) => call(comparator, "$invoke", &[a.clone(), b.clone()]).map_or(std::cmp::Ordering::Equal, |c| c.int().cmp(&0)),
            None => order(a, b),
        }
    }
    fn find(&self, value: &V) -> Result<usize, usize> {
        let items = lock(&self.items).clone();
        items.binary_search_by(|probe| self.compare(probe, value))
    }
    fn insert(&self, value: V) -> bool {
        match self.find(&value) {
            Ok(_) => false,
            Err(at) => {
                lock(&self.items).insert(at, value);
                true
            }
        }
    }
    fn take(&self, value: &V) -> bool {
        match self.find(value) {
            Ok(at) => {
                lock(&self.items).remove(at);
                true
            }
            Err(_) => false,
        }
    }
}

fn sorted_method(sorted_set: &JSorted, this: &V, name: &str, a: &[V]) -> Option<R> {
    let empty = || exc("NoSuchElementException", "empty");
    let copy = || lock(&sorted_set.items).clone();
    Some(match (name, a.len()) {
        ("add", 1) => Ok(V::Bool(sorted_set.insert(a[0].clone()))),
        ("addAll", 1) => match a[0].items() {
            Ok(more) => {
                let mut changed = false;
                for item in more {
                    changed |= sorted_set.insert(item);
                }
                Ok(V::Bool(changed))
            }
            Err(error) => Err(error),
        },
        ("remove", 1) => Ok(V::Bool(sorted_set.take(&a[0]))),
        ("contains", 1) => Ok(V::Bool(sorted_set.find(&a[0]).is_ok())),
        ("size", 0) => Ok(V::Int(lock(&sorted_set.items).len() as i32)),
        ("isEmpty", 0) => Ok(V::Bool(lock(&sorted_set.items).is_empty())),
        ("clear", 0) => {
            lock(&sorted_set.items).clear();
            Ok(V::Null)
        }
        ("first", 0) => lock(&sorted_set.items).first().cloned().ok_or_else(empty),
        ("last", 0) => lock(&sorted_set.items).last().cloned().ok_or_else(empty),
        ("pollFirst", 0) => {
            let mut items = lock(&sorted_set.items);
            Ok(if items.is_empty() { V::Null } else { items.remove(0) })
        }
        ("pollLast", 0) => Ok(lock(&sorted_set.items).pop().unwrap_or(V::Null)),
        ("iterator", 0) => Ok(iterator(this, copy())),
        ("removeIf", 1) => {
            let mut changed = false;
            for item in copy() {
                match call(&a[0], "$invoke", std::slice::from_ref(&item)) {
                    Ok(hit) if hit.truth() => changed |= sorted_set.take(&item),
                    Ok(_) => {}
                    Err(error) => return Some(Err(error)),
                }
            }
            Ok(V::Bool(changed))
        }
        ("comparator", 0) => Ok(sorted_set.comparator.clone().unwrap_or(V::Null)),
        _ => return call(&list(copy()), name, a).into(),
    })
}

/// Sorted collections take a comparator, or what to copy.
pub fn construct(class: &str, a: &[V]) -> Option<R> {
    let is_comparator = |v: &V| matches!(v, V::Func(_) | V::Obj(_));
    match class {
        "TreeSet" | "ConcurrentSkipListSet" => {
            let comparator = a.first().filter(|v| is_comparator(v)).cloned();
            let made = JSorted { items: Mutex::new(Vec::new()), comparator };
            if let Some(source) = a.first().filter(|v| !is_comparator(v)) {
                match source.items() {
                    Ok(items) => {
                        for item in items {
                            made.insert(item);
                        }
                    }
                    Err(error) => return Some(Err(error)),
                }
            }
            Some(Ok(native(made)))
        }
        "TreeMap" | "ConcurrentSkipListMap" if a.first().is_some_and(is_comparator) => Some(Ok(V::Map(Arc::new(Mutex::new(JMap::with_order(crate::Order::Sorted)))))),
        _ => None,
    }
}

/// `list.listIterator()`: a cursor between items of the live list, and the item last returned (what `set` and
/// `remove` act on), as Java's ArrayList.ListItr keeps them.
pub struct JListIterator {
    list: Arc<Mutex<Vec<V>>>,
    cursor: Mutex<usize>,
    last: Mutex<Option<usize>>,
}

fn list_iterator_method(it: &JListIterator, name: &str, a: &[V]) -> Option<R> {
    let mut cursor = lock(&it.cursor);
    let mut last = lock(&it.last);
    let mut list = lock(&it.list);
    Some(match (name, a.len()) {
        ("hasNext", 0) => Ok(V::Bool(*cursor < list.len())),
        ("hasPrevious", 0) => Ok(V::Bool(*cursor > 0)),
        ("nextIndex", 0) => Ok(V::Int(*cursor as i32)),
        ("previousIndex", 0) => Ok(V::Int(*cursor as i32 - 1)),
        ("next", 0) => match list.get(*cursor).cloned() {
            Some(item) => {
                *last = Some(*cursor);
                *cursor += 1;
                Ok(item)
            }
            None => Err(exc("NoSuchElementException", "end")),
        },
        ("previous", 0) => {
            if *cursor == 0 {
                return Some(Err(exc("NoSuchElementException", "start")));
            }
            *cursor -= 1;
            *last = Some(*cursor);
            Ok(list[*cursor].clone())
        }
        ("remove", 0) => match last.take() {
            Some(at) if at < list.len() => {
                list.remove(at);
                if at < *cursor {
                    *cursor -= 1;
                }
                Ok(V::Null)
            }
            _ => Err(exc("IllegalStateException", "next() or previous() first")),
        },
        ("set", 1) => match *last {
            Some(at) if at < list.len() => {
                list[at] = a[0].clone();
                Ok(V::Null)
            }
            _ => Err(exc("IllegalStateException", "next() or previous() first")),
        },
        ("add", 1) => {
            let at = (*cursor).min(list.len());
            list.insert(at, a[0].clone());
            *cursor = at + 1;
            *last = None;
            Ok(V::Null)
        }
        ("forEachRemaining", 1) => {
            let rest: Vec<V> = list[(*cursor).min(list.len())..].to_vec();
            *cursor = list.len();
            *last = rest.len().checked_sub(1).map(|n| *cursor - rest.len() + n);
            drop(list);
            drop(last);
            drop(cursor);
            for item in rest {
                if let Err(e) = call(&a[0], "accept", &[item]) {
                    return Some(Err(e));
                }
            }
            Ok(V::Null)
        }
        _ => return None,
    })
}

fn iterator(source: &V, items: Vec<V>) -> V {
    native(JIterator { source: source.clone(), items, at: Mutex::new(0), removed: Mutex::new(0) })
}

fn view_method(view: &JView, this: &V, name: &str, a: &[V]) -> Option<R> {
    Some(match (name, a.len()) {
        ("iterator", 0) => Ok(iterator(this, snapshot(view))),
        ("remove", 1) => Ok(V::Bool(drop_item(view, &a[0]))),
        ("removeIf", 1) => {
            let mut changed = false;
            for item in snapshot(view) {
                match call(&a[0], "$invoke", std::slice::from_ref(&item)) {
                    Ok(hit) if hit.truth() => changed |= drop_item(view, &item),
                    Ok(_) => {}
                    Err(error) => return Some(Err(error)),
                }
            }
            Ok(V::Bool(changed))
        }
        ("removeAll" | "retainAll", 1) => {
            let keep = name == "retainAll";
            let mut changed = false;
            for item in snapshot(view) {
                let inside = call(&a[0], "contains", std::slice::from_ref(&item)).is_ok_and(|c| c.truth());
                if inside != keep {
                    changed |= drop_item(view, &item);
                }
            }
            Ok(V::Bool(changed))
        }
        ("clear", 0) => {
            lock(&view.map).clear();
            Ok(V::Null)
        }
        ("add" | "addAll", _) => Err(exc("UnsupportedOperationException", "a map's view can't grow")),
        _ => return call(&plain(view), name, a).into(),
    })
}

fn iterator_method(it: &JIterator, this: &V, name: &str, a: &[V]) -> Option<R> {
    let mut at = lock(&it.at);
    Some(match name {
        "hasNext" => Ok(V::Bool(*at < it.items.len())),
        "hasPrevious" => Ok(V::Bool(*at > 0)),
        "nextIndex" => Ok(V::Int(*at as i32)),
        "previousIndex" => Ok(V::Int(*at as i32 - 1)),
        "next" => match it.items.get(*at) {
            Some(item) => {
                *at += 1;
                Ok(item.clone())
            }
            None => Err(exc("NoSuchElementException", "end")),
        },
        "previous" => {
            if *at == 0 {
                return Some(Err(exc("NoSuchElementException", "start")));
            }
            *at -= 1;
            Ok(it.items[*at].clone())
        }
        "remove" => {
            let Some(last) = at.checked_sub(1).and_then(|i| it.items.get(i)) else {
                return Some(Err(exc("IllegalStateException", "next() first")));
            };
            match &it.source {
                V::List(items) => {
                    let mut removed = lock(&it.removed);
                    let index = (*at - 1).saturating_sub(*removed);
                    let mut items = lock(items);
                    if index < items.len() {
                        items.remove(index);
                        *removed += 1;
                    }
                }
                V::Set(map) => {
                    lock(map).remove(&last.key());
                }
                other => {
                    if let Some(view) = other.native::<JView>() {
                        drop_item(view, last);
                    } else if let Some(sorted_set) = other.native::<JSorted>() {
                        sorted_set.take(last);
                    }
                }
            }
            Ok(V::Null)
        }
        "forEachRemaining" => {
            let rest = it.items[*at..].to_vec();
            *at = it.items.len();
            drop(at);
            for item in rest {
                if let Err(error) = call(&a[0], "$invoke", &[item]) {
                    return Some(Err(error));
                }
            }
            Ok(V::Null)
        }
        _ => {
            let _ = this;
            return None;
        }
    })
}

pub(crate) fn order(a: &V, b: &V) -> std::cmp::Ordering {
    match (a, b) {
        (V::Str(x), V::Str(y)) => x.cmp(y),
        (V::Int(_) | V::Long(_) | V::Double(_) | V::Char(_), _) => cmp(a, b),
        _ => call(a, "compareTo", std::slice::from_ref(b)).map_or(std::cmp::Ordering::Equal, |c| c.int().cmp(&0)),
    }
}

fn sorted(mut items: Vec<V>) -> Vec<V> {
    items.sort_by(order);
    items
}

fn near(items: Vec<V>, name: &str, key: &V) -> Option<V> {
    match name {
        "floor" | "floorKey" | "floorEntry" => items.into_iter().rfind(|v| order(v, key).is_le()),
        "lower" | "lowerKey" | "lowerEntry" => items.into_iter().rfind(|v| order(v, key).is_lt()),
        "ceiling" | "ceilingKey" | "ceilingEntry" => items.into_iter().find(|v| order(v, key).is_ge()),
        _ => items.into_iter().find(|v| order(v, key).is_gt()),
    }
}

fn sequence_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    // Copying the collection is for these alone: `get` and `add` are called far too often.
    if !matches!(name, "iterator" | "listIterator" | "descendingIterator" | "removeIf" | "containsAll" | "lastIndexOf" | "removeFirst" | "removeLast" | "first" | "last" | "floor" | "lower" | "ceiling" | "higher" | "headSet" | "tailSet" | "subSet" | "descendingSet") {
        return None;
    }
    let items = match this.items() {
        Ok(items) => items,
        Err(error) => return Some(Err(error)),
    };
    let empty = || exc("NoSuchElementException", "empty");
    Some(match (name, a.len()) {
        ("iterator", 0) => Ok(iterator(this, items)),
        // On a list itself: Java's ListIterator over the live list (ANTLR's IntervalSet removes, steps back and sets).
        ("listIterator", 0 | 1) if matches!(this, V::List(_) | V::Arr(_)) => {
            let (V::List(list) | V::Arr(list)) = this else { unreachable!() };
            let start = a.first().map_or(0, |i| i.int().max(0) as usize).min(items.len());
            Ok(native(JListIterator { list: list.clone(), cursor: Mutex::new(start), last: Mutex::new(None) }))
        }
        ("listIterator", 0) => Ok(iterator(this, items)),
        ("descendingIterator", 0) => Ok(iterator(&V::Null, items.into_iter().rev().collect())),
        ("removeIf", 1) => {
            let mut changed = false;
            let mut drop_list = Vec::new();
            for (index, item) in items.into_iter().enumerate() {
                match call(&a[0], "$invoke", std::slice::from_ref(&item)) {
                    Ok(hit) if hit.truth() => {
                        changed = true;
                        drop_list.push((index, item));
                    }
                    Ok(_) => {}
                    Err(error) => return Some(Err(error)),
                }
            }
            match this {
                V::List(list_items) => {
                    let mut list_items = lock(list_items);
                    for (index, _) in drop_list.iter().rev() {
                        list_items.remove(*index);
                    }
                }
                V::Set(map) => {
                    let mut map = lock(map);
                    for (_, item) in &drop_list {
                        map.remove(&item.key());
                    }
                }
                _ => {}
            }
            Ok(V::Bool(changed))
        }
        ("containsAll", 1) => {
            let wanted = match a[0].items() {
                Ok(wanted) => wanted,
                Err(error) => return Some(Err(error)),
            };
            Ok(V::Bool(wanted.iter().all(|w| call(this, "contains", std::slice::from_ref(w)).is_ok_and(|c| c.truth()))))
        }
        ("lastIndexOf", 1) if matches!(this, V::List(_)) => Ok(V::Int(items.iter().rposition(|v| same(v, &a[0])).map_or(-1, |i| i as i32))),
        ("removeFirst" | "removeLast", 0) if matches!(this, V::List(_)) => {
            let V::List(list_items) = this else { return None };
            let mut list_items = lock(list_items);
            if list_items.is_empty() {
                return Some(Err(empty()));
            }
            Ok(if name == "removeFirst" { list_items.remove(0) } else { list_items.pop().unwrap_or(V::Null) })
        }
        ("first", 0) if matches!(this, V::Set(_)) => sorted(items).first().cloned().ok_or_else(empty),
        ("last", 0) if matches!(this, V::Set(_)) => sorted(items).last().cloned().ok_or_else(empty),
        ("floor" | "lower" | "ceiling" | "higher", 1) if matches!(this, V::Set(_)) => Ok(near(sorted(items), name, &a[0]).unwrap_or(V::Null)),
        ("headSet", 1) => Ok(set_of(&items.into_iter().filter(|v| order(v, &a[0]).is_lt()).collect::<Vec<_>>())),
        ("tailSet", 1) => Ok(set_of(&items.into_iter().filter(|v| order(v, &a[0]).is_ge()).collect::<Vec<_>>())),
        ("subSet", 2) => Ok(set_of(&items.into_iter().filter(|v| order(v, &a[0]).is_ge() && order(v, &a[1]).is_lt()).collect::<Vec<_>>())),
        ("descendingSet", 0) => Ok(list(sorted(items).into_iter().rev().collect())),
        _ => return None,
    })
}

fn entry(pair: &(V, V)) -> V {
    V::Entry(Arc::new(pair.clone()))
}

fn sorted_pairs(map: &Arc<Mutex<JMap>>) -> Vec<(V, V)> {
    let mut pairs: Vec<(V, V)> = lock(map).values().cloned().collect();
    pairs.sort_by(|x, y| order(&x.0, &y.0));
    pairs
}

fn filtered(map: &Arc<Mutex<JMap>>, keep: impl Fn(&V) -> bool) -> V {
    let source = lock(map);
    let mut out: JMap = source.iter().filter(|(_, (k, _))| keep(k)).map(|(k, v)| (k.clone(), v.clone())).collect();
    out.order = source.order;
    drop(source);
    V::Map(Arc::new(Mutex::new(out)))
}

fn map_method(map: &Arc<Mutex<JMap>>, this: &V, name: &str, a: &[V]) -> Option<R> {
    let view = |kind| native(JView { map: map.clone(), kind });
    let empty = || exc("NoSuchElementException", "empty");
    Some(match (name, a.len()) {
        ("entrySet", 0) => Ok(view(Kind::Entries)),
        ("keySet" | "navigableKeySet", 0) => Ok(view(Kind::Keys)),
        ("values", 0) => Ok(view(Kind::Values)),
        ("firstKey", 0) => sorted_pairs(map).first().map(|p| p.0.clone()).ok_or_else(empty),
        ("lastKey", 0) => sorted_pairs(map).last().map(|p| p.0.clone()).ok_or_else(empty),
        ("firstEntry", 0) => Ok(sorted_pairs(map).first().map_or(V::Null, entry)),
        ("lastEntry", 0) => Ok(sorted_pairs(map).last().map_or(V::Null, entry)),
        // fastutil's linked maps (FAWE's chunk queue, a `Long2ObjectLinkedOpenHashMap`): the first or last key in
        // put order, and `removeFirst`/`removeLast` take that entry out and give its value.
        ("firstLongKey" | "firstIntKey" | "lastLongKey" | "lastIntKey" | "removeFirst" | "removeLast", 0) if !matches!(lock(map).order, crate::jmap::Order::Sorted) => {
            let mut m = lock(map);
            let last = name.starts_with("last") || name == "removeLast";
            let found = if last { m.iter().last().map(|(k, p)| (k.clone(), p.clone())) } else { m.iter().next().map(|(k, p)| (k.clone(), p.clone())) };
            match found {
                None => Err(empty()),
                Some((key, pair)) if name.starts_with("remove") => {
                    m.remove(&key);
                    Ok(pair.1)
                }
                Some((_, pair)) => Ok(pair.0),
            }
        }
        ("pollFirstEntry" | "pollLastEntry", 0) => {
            let pairs = sorted_pairs(map);
            let chosen = if name == "pollFirstEntry" { pairs.first() } else { pairs.last() };
            Ok(chosen.map_or(V::Null, |pair| {
                lock(map).remove(&pair.0.key());
                entry(pair)
            }))
        }
        ("floorKey" | "lowerKey" | "ceilingKey" | "higherKey", 1) => Ok(near(sorted_pairs(map).into_iter().map(|p| p.0).collect(), name, &a[0]).unwrap_or(V::Null)),
        ("floorEntry" | "lowerEntry" | "ceilingEntry" | "higherEntry", 1) => {
            let key = near(sorted_pairs(map).into_iter().map(|p| p.0).collect(), name, &a[0]);
            Ok(key.and_then(|k| lock(map).get(&k.key()).map(entry)).unwrap_or(V::Null))
        }
        ("headMap", 1) => Ok(filtered(map, |k| order(k, &a[0]).is_lt())),
        ("tailMap", 1) => Ok(filtered(map, |k| order(k, &a[0]).is_ge())),
        ("subMap", 2) => Ok(filtered(map, |k| order(k, &a[0]).is_ge() && order(k, &a[1]).is_lt())),
        ("descendingMap", 0) => Ok(this.clone()),
        ("descendingKeySet", 0) => Ok(list(sorted_pairs(map).into_iter().rev().map(|p| p.0).collect())),
        ("hashCode", 0) => {
            let pairs: Vec<(V, V)> = lock(map).values().cloned().collect();
            let hash = |v: &V| if v.is_null() { 0 } else { call(v, "hashCode", &[]).map_or(0, |h| h.int()) };
            Ok(V::Int(pairs.iter().fold(0, |sum: i32, (k, v)| sum.wrapping_add(hash(k) ^ hash(v)))))
        }
        ("remove", 2) => {
            let mut map = lock(map);
            let matches = map.get(&a[0].key()).is_some_and(|(_, v)| same(v, &a[1]) || equals(v, &a[1]).is_ok_and(|e| e.truth()));
            if matches {
                map.remove(&a[0].key());
            }
            Ok(V::Bool(matches))
        }
        ("replace", 2) => {
            let mut map = lock(map);
            Ok(match map.get_mut(&a[0].key()) {
                Some(pair) => std::mem::replace(&mut pair.1, a[1].clone()),
                None => V::Null,
            })
        }
        _ => return None,
    })
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    match this {
        V::Map(map) => map_method(map, this, name, a),
        V::Set(items) if name == "hashCode" && a.is_empty() => {
            let keys: Vec<V> = lock(items).values().map(|(k, _)| k.clone()).collect();
            Some(Ok(V::Int(keys.iter().fold(0, |sum: i32, k| sum.wrapping_add(call(k, "hashCode", &[]).map_or(0, |h| h.int()))))))
        }
        V::List(_) | V::Set(_) => sequence_method(this, name, a),
        V::Func(_) => match (name, a.len()) {
            ("equals", 1) => Some(Ok(V::Bool(matches!((this, &a[0]), (V::Func(x), V::Func(y)) if Arc::ptr_eq(x, y))))),
            ("hashCode", 0) => Some(Ok(V::Int(if let V::Func(f) = this { Arc::as_ptr(f).cast::<()>() as usize as i32 } else { 0 }))),
            _ => None,
        },
        V::Stream(items) => match (name, a.len()) {
            ("parallel" | "sequential" | "unordered" | "onClose", _) => Some(Ok(this.clone())),
            ("forEachOrdered", 1) => Some(call(this, "forEach", a)),
            ("takeWhile" | "dropWhile", 1) => {
                let mut out = Vec::new();
                let mut open = true;
                for item in lock(items).clone() {
                    let hit = match call(&a[0], "$invoke", std::slice::from_ref(&item)) {
                        Ok(hit) => hit.truth(),
                        Err(error) => return Some(Err(error)),
                    };
                    open = open && hit;
                    if open == (name == "takeWhile") {
                        out.push(item);
                    }
                }
                Some(Ok(V::Stream(Arc::new(Mutex::new(out)))))
            }
            _ => None,
        },
        V::Opt(value) if name == "flatMap" => Some(match value.as_ref() {
            Some(inner) => call(&a[0], "$invoke", std::slice::from_ref(inner)),
            None => Ok(this.clone()),
        }),
        native_value => {
            if let Some(view) = native_value.native::<JView>() {
                return view_method(view, this, name, a);
            }
            if let Some(sorted_set) = native_value.native::<JSorted>() {
                return sorted_method(sorted_set, this, name, a);
            }
            if let Some(it) = native_value.native::<JIterator>() {
                return iterator_method(it, this, name, a);
            }
            if let Some(it) = native_value.native::<JListIterator>() {
                return list_iterator_method(it, name, a);
            }
            None
        }
    }
}

pub fn is_collection(value: &V, name: &str) -> bool {
    value.native::<JView>().is_some_and(|view| match name {
        "Collection" | "Iterable" => true,
        "Set" => view.kind != Kind::Values,
        _ => false,
    }) || ((value.native::<JIterator>().is_some() || value.native::<JListIterator>().is_some()) && matches!(name, "Iterator" | "ListIterator"))
        || (value.native::<JSorted>().is_some() && matches!(name, "Set" | "SortedSet" | "NavigableSet" | "TreeSet" | "Collection" | "Iterable"))
}
