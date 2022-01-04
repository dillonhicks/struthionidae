use symbol_tree::str::Str;
use symbol_tree::graph::Node;

fn main() {
    let s = Str::from_str("hello world");
    println!("{}", s.clone());

    #[derive(Debug)]
    struct Data {
        id: usize,
        name: Str,
    }

    let n = Node::new(Data {id: 0, name: Str::from_static("XuluBravoTango")}, None);
    println!("{:?}", n);

    n.add_child(Data{id: 2, name: Str::from_static("dillon")});

    println!("{:#?}", n);


}
