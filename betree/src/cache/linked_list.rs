use gxhash::HashMap;
use std::hash::Hash;

const NULL: usize = usize::MAX;

struct Node<K> {
    key: Option<K>,
    prev: usize,
    next: usize,
}

/// A $O(1)$ slab-based doubly linked list.
/// It uses a `Vec` for memory locality and index-based pointers to avoid fragmentation.
pub struct LinkedKeySet<K> {
    nodes: Vec<Node<K>>,
    head: usize,
    tail: usize,
    free_head: usize,
    map: HashMap<K, usize>,
}

impl<K: Clone + Eq + Hash> LinkedKeySet<K> {
    /// Creates a new, empty `LinkedKeySet`.
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            head: NULL,
            tail: NULL,
            free_head: NULL,
            map: HashMap::default(),
        }
    }

    /// Pushes a key to the front (MRU). If it already exists, it is moved to the front.
    pub fn push_front(&mut self, key: K) {
        if self.map.contains_key(&key) {
            self.move_to_front(&key);
            return;
        }
        let idx = self.allocate_node(key.clone());
        self.link_front(idx);
        self.map.insert(key, idx);
    }

    /// Removes a key from the set. Returns `true` if it was present.
    pub fn remove(&mut self, key: &K) -> bool {
        if let Some(idx) = self.map.remove(key) {
            self.unlink(idx);
            self.free_node(idx);
            true
        } else {
            false
        }
    }

    /// Removes the least recently used element (the tail) and returns it.
    pub fn pop_back(&mut self) -> Option<K> {
        if self.tail == NULL {
            return None;
        }
        let idx = self.tail;
        self.unlink(idx);
        let key = self.free_node(idx);
        self.map.remove(&key);
        Some(key)
    }

    /// Checks if the key is present.
    pub fn contains(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// Returns the number of elements in the set.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns an iterator that yields elements from Tail (LRU) to Head (MRU).
    pub fn iter_lru(&self) -> IterLru<K> {
        IterLru {
            set: self,
            curr: self.tail,
        }
    }

    fn move_to_front(&mut self, key: &K) {
        if let Some(&idx) = self.map.get(key) {
            self.unlink(idx);
            self.link_front(idx);
        }
    }

    fn allocate_node(&mut self, key: K) -> usize {
        if self.free_head != NULL {
            let idx = self.free_head;
            self.free_head = self.nodes[idx].next;
            self.nodes[idx].key = Some(key);
            idx
        } else {
            let idx = self.nodes.len();
            self.nodes.push(Node {
                key: Some(key),
                prev: NULL,
                next: NULL,
            });
            idx
        }
    }

    fn free_node(&mut self, idx: usize) -> K {
        let key = self.nodes[idx].key.take().unwrap();
        self.nodes[idx].next = self.free_head;
        self.free_head = idx;
        key
    }

    fn link_front(&mut self, idx: usize) {
        self.nodes[idx].prev = NULL;
        self.nodes[idx].next = self.head;
        if self.head != NULL {
            self.nodes[self.head].prev = idx;
        }
        self.head = idx;
        if self.tail == NULL {
            self.tail = idx;
        }
    }

    fn unlink(&mut self, idx: usize) {
        let prev = self.nodes[idx].prev;
        let next = self.nodes[idx].next;
        if prev != NULL {
            self.nodes[prev].next = next;
        } else {
            self.head = next;
        }
        if next != NULL {
            self.nodes[next].prev = prev;
        } else {
            self.tail = prev;
        }
    }
}

pub struct IterLru<'a, K> {
    set: &'a LinkedKeySet<K>,
    curr: usize,
}

impl<'a, K> Iterator for IterLru<'a, K> {
    type Item = &'a K;

    fn next(&mut self) -> Option<Self::Item> {
        if self.curr == NULL {
            None
        } else {
            let node = &self.set.nodes[self.curr];
            self.curr = node.prev;
            node.key.as_ref()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_front_and_pop_back() {
        let mut list = LinkedKeySet::new();
        list.push_front(1);
        list.push_front(2);
        list.push_front(3);

        assert_eq!(list.len(), 3);

        assert_eq!(list.pop_back(), Some(1));
        assert_eq!(list.pop_back(), Some(2));
        assert_eq!(list.pop_back(), Some(3));
        assert_eq!(list.pop_back(), None);
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn test_remove() {
        let mut list = LinkedKeySet::new();
        list.push_front(1);
        list.push_front(2);
        list.push_front(3);

        assert!(list.remove(&2));
        assert!(!list.remove(&2));
        assert_eq!(list.len(), 2);

        assert_eq!(list.pop_back(), Some(1));
        assert_eq!(list.pop_back(), Some(3));
        assert_eq!(list.pop_back(), None);
    }

    #[test]
    fn test_move_to_front() {
        let mut list = LinkedKeySet::new();
        list.push_front(1);
        list.push_front(2);
        list.push_front(3);

        list.push_front(1);

        assert_eq!(list.len(), 3);

        let items: Vec<&i32> = list.iter_lru().collect();
        assert_eq!(items, vec![&2, &3, &1]);

        assert_eq!(list.pop_back(), Some(2));
        assert_eq!(list.pop_back(), Some(3));
        assert_eq!(list.pop_back(), Some(1));
        assert_eq!(list.pop_back(), None);
    }

    #[test]
    fn test_free_node_reuse() {
        let mut list = LinkedKeySet::new();
        list.push_front(1);
        assert_eq!(list.nodes.len(), 1);

        list.remove(&1);
        assert_eq!(list.nodes.len(), 1);
        assert_eq!(list.free_head, 0);

        list.push_front(2);
        assert_eq!(list.nodes.len(), 1);
        assert_eq!(list.free_head, NULL);
        assert_eq!(list.nodes[0].key, Some(2));
    }

    #[test]
    fn test_iter_lru() {
        let mut list = LinkedKeySet::new();
        list.push_front(1);
        list.push_front(2);
        list.push_front(3);

        let lru: Vec<i32> = list.iter_lru().copied().collect();
        assert_eq!(lru, vec![1, 2, 3]);
    }
}
