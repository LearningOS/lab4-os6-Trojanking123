use alloc::collections::BTreeMap;

use super::{
    BlockDevice,
    DiskInode,
    DiskInodeType,
    DirEntry,
    EasyFileSystem,
    DIRENT_SZ,
    get_block_cache,
    block_cache_sync_all,
};
use alloc::{sync::Arc, borrow::ToOwned};
use alloc::string::String;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};



/// Virtual filesystem layer over easy-fs
pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// Create a vfs inode
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }
    /// Call a function over a disk inode to read it
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(
            self.block_id,
            Arc::clone(&self.block_device)
        ).lock().read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(
            self.block_id,
            Arc::clone(&self.block_device)
        ).lock().modify(self.block_offset, f)
    }
    /// Find inode under a disk inode by name
    fn find_inode_id(
        &self,
        name: &str,
        disk_inode: &DiskInode,
    ) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(
                    DIRENT_SZ * i,
                    dirent.as_bytes_mut(),
                    &self.block_device,
                ),
                DIRENT_SZ,
            );
            if dirent.name() == name && dirent.is_valid() {
                return Some(dirent.inode_number() as u32);
            }
        }
        None
    }
    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode)
            .map(|inode_id| {
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
    }


    /// Find inode under current inode by name
    pub fn link(&self, oldpath: &str, newpath: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        let old_inode_id = self.modify_disk_inode(|root_inode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(oldpath, root_inode)
        });
        if old_inode_id.is_none() {
            return None ;
        }

 
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(newpath, old_inode_id.unwrap());
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(old_inode_id.unwrap());
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
    }

    pub fn unlink(&self, pathname: &str) -> isize {
        let mut fs = self.fs.lock();
        let info = self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(
                        i * DIRENT_SZ,
                        dirent.as_bytes_mut(),
                        &self.block_device,
                    ),
                    DIRENT_SZ,
                );
                
                v.push(dirent);
                
                
            }
            v
        });

        let mut tmp: BTreeMap<u32, Vec<String>> = BTreeMap::new();
        let mut name_id_map: BTreeMap<String, u32> = BTreeMap::new();
        let mut name_index_map: BTreeMap<String, u32> = BTreeMap::new();
        for (index, dir) in info.iter().enumerate()  {
                let dirname = dir.name().to_owned();
                let inode = dir.inode_number();
                name_id_map.insert(dirname.clone(), inode);
                name_index_map.insert(dirname.clone(), index as u32);
                if tmp.contains_key(&inode) {
                    tmp.get_mut(&inode).unwrap().push(dirname);
                }else {
                    let mut v = Vec::new();
                    v.push(dirname);
                    tmp.insert(inode, v);
                }
                tmp.insert(dir.inode_number(), Vec::new());
        }
        
        
        if let Some(inode_id) = name_id_map.get(&pathname.to_owned()) {
    
            if tmp[inode_id].len() == 1 {
                let (block_id, block_offset) = fs.get_disk_inode_pos(*inode_id);
                    block_cache_sync_all();
                    // return inode
                    let inode = Arc::new(Self::new(
                        block_id,
                        block_offset,
                        self.fs.clone(),
                        self.block_device.clone(),
                    ));
                    inode.clear();
                    fs.dealloc_inode(*inode_id);
            }

            self.modify_disk_inode(|root_inode| {
                
                let index = *name_index_map.get(&pathname.to_owned()).unwrap() as usize ;
                log::info!("unlink modify index {:?}", index);
                // write dirent
                let mut dirent = info[index].clone();
                dirent.set_valid(1);
                log::info!("unlink cover dirrent {:?}", dirent.name().clone());
                root_inode.write_at(
                    index * DIRENT_SZ,
                    dirent.as_bytes(),
                    &self.block_device,
                );
            });
            block_cache_sync_all();
            return 0;
        }else {
            return -1;
        }

    }

    pub fn fstat(&self) -> (u64, u32)  {
        
        let efs = self.fs.clone();
        let root_inode = Arc::new(EasyFileSystem::root_inode(&efs));

        let info = root_inode.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: BTreeMap<u32, u32> = BTreeMap::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(
                        i * DIRENT_SZ,
                        dirent.as_bytes_mut(),
                        &root_inode.block_device,
                    ),
                    DIRENT_SZ,
                );
                if dirent.is_valid() {
                    log::info!("fstat got file index: {:?} name: {:?} inodeid {:?}",i, dirent.name().clone(), dirent.inode_number());
                    if let Some(v) =  v.get_mut(&dirent.inode_number()) {
                        *v += 1;
                    }else {
                        v.insert(dirent.inode_number(), 1);
                    }
                }
                
            }
            v
        });
        let fs = self.fs.lock();
        for (k, v) in info {
            let (a, b) = fs.get_disk_inode_pos(k);
            if a as usize == self.block_id && b == self.block_offset {
                return (k as u64, v);
            }
            
        }

    (0, 0)
    }

    
    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }
    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        if self.modify_disk_inode(|root_inode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        }).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) 
            = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(
            new_inode_block_id as usize,
            Arc::clone(&self.block_device)
        ).lock().modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
            new_inode.initialize(DiskInodeType::File);
        });
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(name, new_inode_id);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
        // release efs lock automatically by compiler
    }
    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(
                        i * DIRENT_SZ,
                        dirent.as_bytes_mut(),
                        &self.block_device,
                    ),
                    DIRENT_SZ,
                );
                if dirent.is_valid() {
                    v.push(String::from(dirent.name()));
                }
                
            }
            v
        })
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            disk_inode.read_at(offset, buf, &self.block_device)
        })
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }
}
