use std::fmt;
use crate::str::Str;
use crate::shared::{Shared, WeaklyShared};
use crate::deps::owning_ref::RefRef;

type Result<T> = std::result::Result<T, ()>;


type Id = usize;

struct WeakNode<T> {
    inner: WeaklyShared<Inner<T>>
}

impl<T> WeakNode<T> {
    fn upgrade(&self) -> Option<Node<T>> {
        self.inner.upgrade().map(|inner| Node { inner })
    }

    fn dangling() -> Self {
        Self {
            inner: WeaklyShared::dangling()
        }
    }
}

impl<T> Default for WeakNode<T> {
    fn default() -> Self {
        Self::dangling()
    }
}

impl<T> From<&'_ Node<T>> for WeakNode<T> {
    fn from(n: &Node<T>) -> Self {
        dbg!("WEAKNODE CREATE");
        WeakNode {
            inner: WeaklyShared::downgrade(&n.inner)
        }
    }
}


#[derive(Default)]
pub struct Inner<T> {
    id: usize,
    parent: WeakNode<T>,
    data: T,
    children: Vec<Node<T>>,
}


pub type FieldRef<'a, T, V> = RefRef<'a, Inner<T>, V>;

static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub struct Node<T> {
    inner: Shared<Inner<T>>
}

impl<T> Clone for Node<T> {
    fn clone(&self) -> Self {
        Node {
            inner: self.inner.clone()
        }
    }
}

impl<T> Node<T> {
    pub fn new(data: T, parent: Option<&Node<T>>) -> Self {
        Node {
            inner: Shared::new(Inner {
                id: COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
                parent: parent.map(WeakNode::from).unwrap_or_default(),
                children: Default::default(),
                data,
            })
        }
    }

    fn refref(&self) -> FieldRef<'_, T, Inner<T>> {
        RefRef::new(self.inner.read())
    }

    pub fn children(&self) -> FieldRef<'_, T, [Node<T>]> {
        let refref = RefRef::new(self.inner.read());
        refref.map(|inner| &inner.children[..])
    }


    pub fn id(&self) -> Id {
        self.inner.read().id
    }

    pub fn parent(&self) -> Option<Node<T>> {
        self.inner.read().parent.upgrade()
    }

    pub fn data(&self) -> FieldRef<'_, T, T> {
        self.refref().map(|inner| &inner.data)
    }

    pub fn add_child(&self, data: T) -> Node<T> {
        let mut inner = self.inner.write();
        let idx = inner.children.len();
        inner.children.push(Node::new(data, Some(self)));
        inner.children[idx].clone()
    }

}


impl<T> fmt::Debug for Node<T> where T: fmt::Debug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.inner.read();


        let mut f = f.debug_struct("Node");
        f.field("id", &format_args!("{:x}", self.id()))
            .field("data", &inner.data);

        if let Some(parent) = inner.parent.upgrade() {
            f.field("parent", &format_args!("{:x}", parent.id()));
        } else {
            f.field("parent", &Option::<()>::None);
        }

        f
            .field("children", &inner.children)
            .finish()
    }
}


pub fn visit_level_order<F, T>(root: Node<T>, mut visitor: F) where F: (FnMut(i32, &Node<T>) -> ()) {
    let mut queue = std::collections::VecDeque::new();
    queue.push_back((0, root));

    while let Some((level, node)) = queue.pop_front() {
        visitor(level, &node);
        for child in node.children().iter() {
            queue.push_back((level + 1, child.clone()))
        }
    }
}


pub fn visit_in_order<F, T>(root: Node<T>, mut visitor: F) where F: (FnMut(i32, &Node<T>) -> ()) {
    let mut queue = std::collections::VecDeque::new();
    queue.push_back((0, root));

    while let Some((level, node)) = queue.pop_back() {
        visitor(level, &node);
        for child in node.children().iter() {
            queue.push_back((level + 1, child.clone()))
        }
    }
}

pub fn write_list<W, T>(mut w: W, root: &Node<T>) -> std::io::Result<()> where W: std::io::Write {
    visit_level_order(root.clone(), |level, next| {
        for _ in 0..(level + 1) {
            write!(w, "--").unwrap();
        }
        writeln!(w, "> {}", next.id());
    });

    Ok(())
}


pub fn walk<F, T>(mut node: Node<T>, mut visitor: F) where F: (FnMut(&Node<T>) -> ()) ,T: fmt::Debug {
    visitor(&node);

    while let Some(parent) = node.parent() {
        visitor(&parent);
        node = parent;
    }
}


#[cfg(test)]
mod tests {
    use crate::str::Str;
    use crate::graph::{Node, write_list, visit_in_order, walk};
    use std::collections::{VecDeque, HashSet, HashMap};

    #[test]
    fn test_path() {
        #[derive(Debug)]
        struct Data {
            id: usize,
            name: Str,
        }

        let n = Node::new(Data { id: 0, name: Str::from_static("XuluBravoTango") }, None);
        println!("{:?}", n);

        for i in 0..10 {
            let n = n.add_child(Data { id: 1 + (10 * i), name: Str::from_static("X") });
            for j in 1..=9 {
                n.add_child(Data { id: 1 + (10 * i) + j, name: Str::from_static("X") });
            }
        }
        println!("{:?}", n);

        println!("\n\n\n");
        let mut stdout = std::io::stdout();

        write_list(stdout.lock(), &n);
    }

    #[test]
    fn test_quadtree() -> Result<(), Box<dyn std::error::Error>>{
        type Code = u8;

        #[derive(Debug)]
        struct Data {
            level: usize,
            loc: (f32, f32),
            code: Code,
        }

        const MAX_LEVEL: usize = 3;

        let max_loc = 2.0f32.powf(MAX_LEVEL as f32) - 1.0f32;
        let mid = max_loc / 2.0f32;
        let root = Node::new(Data { level: 0, loc: (mid, mid), code: 0b00 }, None);
        println!("{:?}", root);

        let mut queue = VecDeque::new();
        queue.push_back(root.clone());

        while let Some(node) = queue.pop_front() {
            let next_level = node.data().level + 1;
            let (x, y) = node.data().loc;
            let parent_code = node.data().code;
            let shift = (Code::max_value().count_ones()).checked_sub(next_level as u32 * 2).unwrap_or_default();

            if next_level <= MAX_LEVEL {
                let delta = 2.0f32.powf(MAX_LEVEL as f32) / (2.0f32.powf( 1f32 + next_level as f32));
                //let delta = if delta < 1.0f32 { 1.0f32 } else { delta};
                queue.push_back(node.add_child(Data { level: next_level, loc: (x + delta, y + delta), code: parent_code | (0b11 << shift) }));
                queue.push_back(node.add_child(Data { level: next_level, loc: (x - delta, y + delta), code: parent_code | (0b10 << shift) }));
                queue.push_back(node.add_child(Data { level: next_level, loc: (x + delta, y - delta),  code: parent_code | (0b01 << shift) }));
                queue.push_back(node.add_child(Data { level: next_level, loc: (x - delta, y - delta),  code: parent_code | (0b00 << shift)  }));
            }
        }

        //println!("{:?}", root);

        println!("\n\n\n");
        let mut w = std::io::stdout();

        fn format_code(code: Code) -> String {
            const MASK: Code = 0b11;
            const BITS: u32 = Code::max_value().count_ones();
            let mut bits = 0;
            let num = std::iter::from_fn(|| {
                bits += 2;
                if bits <= BITS {
                    let shift = BITS - bits;
                    Some((shift, MASK << shift))
                } else {
                    None
                }
            }).map(|(shift, mask)| (code & mask) >> shift)
                .map(|num| format!("{:0>2b}", num))
                .collect::<Vec<_>>();

            format!("0b{}", num.join("_"))
        }

        let mut count = 0;
        let cloned = root.clone();


        visit_in_order(cloned, |level, next| {

            if !next.children().is_empty() {
                return;
            }
            use std::io::Write;

            for _ in 0..(level + 1) {
                write!(w, "--").unwrap();
            }

            let code = next.data().code;
            let loc = next.data().loc;
            writeln!(w, "> {:>3}: {} {} {:?}", count, format_code(code), code, loc);
            count += 1;
        });


        println!("\n\n\n\n");


        println!("const GRID16_BY_XYS: [(u8, u8); 256] =[");

        let mut codes_and_locs = vec![];

        visit_in_order(root.clone(), |level, next| {
            use std::io::Write;

            if !next.children().is_empty() {
                return;
            }

            let code = next.data().code;
            let (x, y) = next.data().loc;
            let loc = (x as i32, y as i32);
            println!("    // {} ({})\n    {:?},", format_code(code), code,  loc);
            codes_and_locs.push((loc, code));

            count += 1;
        });

        println!("]");

        use crate::deps::itertools::Itertools;

        codes_and_locs.sort_unstable_by_key(|((x, y), _code)| (*x, *y));

        println!("const GRID16_BY_XYS: [[u8; 16]; 16] =[");
        for (key, group) in &codes_and_locs.into_iter()
            .group_by(|((x, _y), _code)| *x) {

            print!("    [");
            for ((x, y), code)  in group {
                print!(" {},", format_code(code));
            }
            println!("],");
            //println!("{:?} {:?}", key, group.collect::<Vec<_>>());
        }
        println!("];");

        //let mut locs = HashMap::new();

        Ok(())
    }
}

