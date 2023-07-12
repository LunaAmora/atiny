use std::{collections::HashMap, error::Error, io, path::PathBuf, rc::Rc};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(pub usize);

pub struct Dir {
    pub id: NodeId,
    parent: Option<NodeId>,
    files: HashMap<String, NodeId>,
}

impl Dir {
    fn new(id: NodeId, parent: Option<NodeId>) -> Self {
        Self {
            id,
            parent,
            files: HashMap::new(),
        }
    }

    pub fn find(&self, name: &str) -> Option<NodeId> {
        self.files.get(name).copied()
    }

    pub fn get_file_name(&self, id: NodeId) -> Option<&str> {
        self.files
            .iter()
            .find(|(_, &node)| node == id)
            .map(|(name, _)| name.as_str())
    }
}

impl Dir {
    pub fn add_node(&mut self, node_name: String, node: NodeId) {
        self.files.insert(node_name, node);
    }
}
#[derive(Clone)]
pub struct File {
    pub id: NodeId,
    pub code: Rc<str>,
    parent: Option<NodeId>,
}

pub enum Node {
    File(File),
    Dir(Dir),
}

impl Node {
    pub fn parent(&self) -> Option<NodeId> {
        match self {
            Self::File(f) => f.parent,
            Self::Dir(d) => d.parent,
        }
    }
}

pub trait VirtualFileSystem<T: VirtualFile, E: Error> {
    fn add_file(&mut self, path: &T, name: String, parent: Option<NodeId>) -> Result<NodeId, E>;
    fn add_dir(&mut self, name: String, parent: Option<NodeId>) -> Result<NodeId, E>;
    fn add_node(&mut self, node: Node, name: String, parent: Option<NodeId>) -> Result<NodeId, E>;

    fn get_file_relative(&mut self, path: &str, node_id: NodeId) -> Result<File, E>;
    fn get_file_from_path(&mut self, path: &str, parent: Option<NodeId>) -> Result<File, E>;
    fn read_file(&mut self, path: &str, parent: Option<NodeId>) -> Result<File, E>;

    fn get_file(&self, node_id: NodeId) -> Option<File>;
    fn get_parent(&self, parent: Option<NodeId>) -> &Dir;
    fn get_root(&self) -> &Dir;

    fn get_node_full_name(&self, node_id: NodeId) -> String;
}

pub trait Reader<T> = Fn(&T) -> io::Result<Rc<str>>;

pub struct FileSystem<'r, T: VirtualFile> {
    root: String,
    nodes: Vec<Node>,
    reader: Box<dyn Reader<T> + 'r>,
}

impl<'r, T: VirtualFile> FileSystem<'r, T> {
    pub fn new(path: &T, reader: impl Reader<T> + 'r) -> Self {
        let root = Node::Dir(Dir::new(NodeId(0), None));
        Self {
            root: path.dir_name(),
            nodes: vec![root],
            reader: Box::new(reader),
        }
    }

    pub fn new_with(path: &T, reader: impl Reader<T> + 'r) -> io::Result<(Self, NodeId)> {
        let mut vfs = Self::new(path, reader);
        vfs.add_file(path, path.file_name(), None)
            .map(|id| (vfs, id))
    }
}

impl<'r, T: VirtualFile> VirtualFileSystem<T, io::Error> for FileSystem<'r, T> {
    fn add_file(&mut self, path: &T, name: String, parent: Option<NodeId>) -> io::Result<NodeId> {
        let file = File {
            id: NodeId(self.nodes.len()),
            parent,
            code: (self.reader)(path)?,
        };

        self.add_node(Node::File(file), name, parent)
    }

    fn add_dir(&mut self, name: String, parent: Option<NodeId>) -> io::Result<NodeId> {
        let dir = Dir::new(NodeId(self.nodes.len()), parent);
        self.add_node(Node::Dir(dir), name, parent)
    }

    fn add_node(&mut self, node: Node, name: String, parent: Option<NodeId>) -> io::Result<NodeId> {
        let id = NodeId(self.nodes.len());

        let parent_node = match parent {
            Some(NodeId(id)) => &mut self.nodes[id],
            _ => &mut self.nodes[0],
        };

        let Node::Dir(dir) = parent_node else {
            panic!("ICE: tried to add a node as a child of a file");
        };

        dir.add_node(name, id);
        self.nodes.push(node);
        Ok(id)
    }

    fn get_file_relative(&mut self, path: &str, NodeId(id): NodeId) -> io::Result<File> {
        self.get_file_from_path(path, self.nodes[id].parent())
    }

    fn get_file_from_path(&mut self, path: &str, parent: Option<NodeId>) -> io::Result<File> {
        let mut qualifiers = path.split('.');
        let mut dir = self.get_parent(parent);

        while let Some(ty) = qualifiers.next() {
            match dir
                .find(&T::from_string(ty).file_name())
                .or_else(|| dir.find(ty))
            {
                Some(NodeId(id)) => {
                    match &self.nodes[id] {
                        Node::Dir(node_dir) => dir = node_dir,
                        Node::File(_) if qualifiers.next().is_some() => {
                            panic!("ICE: tried to treat a file as a directory")
                        }
                        Node::File(f) => return Ok(f.clone()),
                    };
                }
                None => break,
            }
        }

        self.read_file(path, parent)
    }

    fn read_file(&mut self, path: &str, parent: Option<NodeId>) -> io::Result<File> {
        let dir_path = match parent {
            Some(id) => self.get_node_full_name(id),
            None => self.root.clone(),
        };

        let joined_path = T::join_with_file(&dir_path, path);

        let NodeId(id) = self.add_file(&joined_path, joined_path.file_name(), parent)?;
        let Node::File(file) = &self.nodes[id] else {
            unreachable!();
        };

        Ok(file.clone())
    }

    fn get_file(&self, NodeId(id): NodeId) -> Option<File> {
        match &self.nodes[id] {
            Node::File(file) => Some(file.clone()),
            Node::Dir(_) => None,
        }
    }

    fn get_parent(&self, parent: Option<NodeId>) -> &Dir {
        match parent {
            Some(NodeId(id)) => {
                let Node::Dir(dir) = &self.nodes[id] else {
                    panic!("ICE: tried to treat a file as a directory");
                };
                dir
            }
            _ => self.get_root(),
        }
    }

    fn get_root(&self) -> &Dir {
        match &self.nodes[0] {
            Node::Dir(root) => root,
            _ => unreachable!(),
        }
    }

    fn get_node_full_name(&self, NodeId(id): NodeId) -> String {
        match self.nodes[id].parent() {
            None if id == 0 => self.root.to_owned(),

            None => self
                .get_root()
                .get_file_name(NodeId(id))
                .unwrap()
                .to_string(),

            Some(parent_id) => {
                let parent = self.get_node_full_name(parent_id);
                let Node::Dir(dir) = &self.nodes[parent_id.0] else {
                    unreachable!();
                };

                let name = dir.get_file_name(NodeId(id)).unwrap();
                format!("{parent}/{name}")
            }
        }
    }
}

pub trait VirtualFile {
    fn file_name(&self) -> String;
    fn dir_name(&self) -> String;
    fn from_string(file: &str) -> Self;
    fn join_with_file(path: &str, other_path: &str) -> Self;
}

impl VirtualFile for PathBuf {
    fn file_name(&self) -> String {
        self.with_extension("at")
            .as_path()
            .file_name()
            .map(|name| name.to_ascii_lowercase())
            .unwrap()
            .to_string_lossy()
            .to_string()
    }

    fn dir_name(&self) -> String {
        self.parent().unwrap().to_string_lossy().to_string()
    }

    fn from_string(file: &str) -> Self {
        Self::from(file)
    }

    fn join_with_file(path: &str, other_path: &str) -> Self {
        Self::from(path)
            .join(other_path.to_ascii_lowercase())
            .with_extension("at")
    }
}
