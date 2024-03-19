//! Merkelized map and associated proof

use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    hash::Hash,
    rc::Rc,
};

use crate::{StdError, StdResult};

use super::{MKProof, MKTree, MKTreeNode};

/// The trait implemented by the keys of a MKMap
pub trait MKMapKey: PartialEq + Eq + PartialOrd + Ord + Clone + Hash + Into<MKTreeNode> {}

/// The trait implemented by the values of a MKMap
pub trait MKMapValue<K: MKMapKey>: Clone + TryInto<MKTreeNode> {
    /// Get the root of the merkelized map value
    fn compute_root(&self) -> StdResult<MKTreeNode>;

    /// Check if the merkelized map value contains a leaf
    fn contains<T: Into<MKTreeNode> + Clone>(&self, leaf: &T) -> bool;

    /// Compute the proof for a set of values of the merkelized map
    fn compute_proof<T: Into<MKTreeNode> + Clone>(
        &self,
        leaves: &[T],
    ) -> StdResult<Option<MKMapProof<K>>>;
}

/// A map, where the keys and values are merkelized and provable
pub struct MKMap<K: MKMapKey, V: MKMapValue<K>> {
    inner_map_values: BTreeMap<K, V>,
    inner_merkle_tree: MKTree,
}

impl<K: MKMapKey, V: MKMapValue<K>> MKMap<K, V> {
    /// MKMap factory
    pub fn new(entries: &[(K, V)]) -> StdResult<Self> {
        let inner_map_values = BTreeMap::default();
        let inner_merkle_tree = MKTree::new::<MKTreeNode>(&[])?;
        let mut mk_map = Self {
            inner_map_values,
            inner_merkle_tree,
        };
        let sorted_entries = BTreeMap::from_iter(entries.to_vec());
        for (key, value) in sorted_entries {
            mk_map.insert_unchecked(key.clone(), value.clone())?;
        }

        Ok(mk_map)
    }

    /// Insert a new key-value pair
    /// Important: keys must be inserted in order to guarantee
    /// that the same set of key/values results in the same computation for the root.
    pub fn insert(&mut self, key: K, value: V) -> StdResult<()> {
        if let Some(existing_value) = self.inner_map_values.get(&key) {
            if existing_value.compute_root()? != value.compute_root()? {
                return Err(anyhow!(
                    "MKMap values should be replaced by entry with same root"
                ));
            }
        } else {
            let key_max = self.inner_map_values.keys().max();
            if key_max > Some(&key) {
                return Err(anyhow!("MKMap keys must be inserted in order"));
            }
        }

        self.insert_unchecked(key, value)
    }

    /// Insert a new key-value pair without checking if the key is already present nor the order of insertion.
    fn insert_unchecked(&mut self, key: K, value: V) -> StdResult<()> {
        self.inner_map_values.insert(key.clone(), value.clone());
        let mktree_node_value = value
            .try_into()
            .map_err(|_| anyhow!("MKMap could not convert value to NKTreeNode"))
            .with_context(|| "MKMap could not convert insert value")?;
        let mktree_node_key: MKTreeNode = key.into();
        self.inner_merkle_tree
            .append(&[mktree_node_key + mktree_node_value])?;

        Ok(())
    }

    /// Check if the merkelized map contains a leaf (and returns the corresponding key and value if exists)
    pub fn contains(&self, leaf: &MKTreeNode) -> Option<(&K, &V)> {
        self.inner_map_values.iter().find(|(_, v)| v.contains(leaf))
    }

    /// Get the value of the merkelized map for a given key
    pub fn get(&self, key: &K) -> Option<&V> {
        self.inner_map_values.get(key)
    }

    /// Get an iterator for the key and values of the merkelized map
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.inner_map_values.iter()
    }

    /// Get the length of the merkelized map
    pub fn len(&self) -> usize {
        self.inner_map_values.len()
    }

    /// Check if the merkelized map is empty
    pub fn is_empty(&self) -> bool {
        self.inner_map_values.is_empty()
    }

    /// Get the root of the merkle tree of the merkelized map
    pub fn compute_root(&self) -> StdResult<MKTreeNode> {
        self.inner_merkle_tree.compute_root()
    }

    /// Get the proof for a set of values of the merkelized map (recursively if needed)
    pub fn compute_proof<T: Into<MKTreeNode> + Clone>(
        &self,
        leaves: &[T],
    ) -> StdResult<MKMapProof<K>> {
        if leaves.is_empty() {
            return Err(anyhow!("MKMap could not compute proof for empty leaves"));
        }
        let leaves_by_keys: HashMap<K, Vec<MKTreeNode>> = leaves
            .iter()
            .filter_map(|leaf| match self.contains(&leaf.to_owned().into()) {
                Some((key, _)) => Some((key.to_owned(), leaf)),
                _ => None,
            })
            .fold(HashMap::default(), |mut acc, (key, leaf)| {
                acc.entry(key.to_owned())
                    .or_default()
                    .push(leaf.to_owned().into());
                acc
            });

        let mut sub_proofs = BTreeMap::<K, MKMapProof<K>>::default();
        for (key, sub_leaves) in leaves_by_keys {
            if let Some(value) = self.inner_map_values.get(&key) {
                if let Some(proof) = value.compute_proof(&sub_leaves)? {
                    sub_proofs.insert(key.to_owned(), proof);
                }
            }
        }

        let master_proof = self
            .inner_merkle_tree
            .compute_proof(
                &sub_proofs
                    .iter()
                    .map(|(k, p)| k.to_owned().into() + p.compute_root().to_owned())
                    .collect::<Vec<MKTreeNode>>(),
            )
            .with_context(|| "MKMap could not compute master proof")?;

        Ok(MKMapProof::new(master_proof, sub_proofs))
    }
}

impl<K: MKMapKey, V: MKMapValue<K>> Clone for MKMap<K, V> {
    fn clone(&self) -> Self {
        // Cloning should never fail so uwnrap is safe
        let mut clone = Self::new(&[]).unwrap();
        for (k, v) in self.inner_map_values.iter() {
            clone.insert(k.to_owned(), v.to_owned()).unwrap();
        }

        clone
    }
}

impl<'a, K: MKMapKey, V: MKMapValue<K>> From<&'a MKMap<K, V>> for &'a MKTree {
    fn from(other: &'a MKMap<K, V>) -> Self {
        &other.inner_merkle_tree
    }
}

impl<K: MKMapKey, V: MKMapValue<K>> TryFrom<MKMap<K, V>> for MKTreeNode {
    type Error = StdError;
    fn try_from(other: MKMap<K, V>) -> Result<Self, Self::Error> {
        other.compute_root()
    }
}

/// A MKMapProof that proves membership of an entry in the merkelized map
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MKMapProof<K: MKMapKey> {
    master_proof: MKProof,
    sub_proofs: Vec<(K, MKMapProof<K>)>,
}

impl<K: MKMapKey> MKMapProof<K> {
    /// MKMapProof factory
    pub fn new(master_proof: MKProof, sub_proofs: BTreeMap<K, MKMapProof<K>>) -> Self {
        let sub_proofs = sub_proofs.into_iter().collect();
        Self {
            master_proof,
            sub_proofs,
        }
    }

    /// Get the root of the merkelized map proof
    pub fn compute_root(&self) -> MKTreeNode {
        self.master_proof.root().to_owned()
    }

    /// Verify the merkelized map proof
    pub fn verify(&self) -> StdResult<()> {
        for (_key, proof) in &self.sub_proofs {
            proof
                .verify()
                .with_context(|| "MKMapProof could not verify sub proof")?;
        }

        self.master_proof
            .verify()
            .with_context(|| "MKMapProof could not verify master proof")?;
        if !self.sub_proofs.is_empty() {
            self.master_proof
                .contains(
                    &self
                        .sub_proofs
                        .iter()
                        .map(|(k, p)| k.to_owned().into() + p.compute_root().to_owned())
                        .collect::<Vec<_>>(),
                )
                .with_context(|| "MKMapProof could not match verified leaves of master proof")?;
        }

        Ok(())
    }

    /// Check if the merkelized map proof contains a leaf
    pub fn contains(&self, leaf: &MKTreeNode) -> StdResult<()> {
        let master_proof_contains_leaf = self.master_proof.contains(&[leaf.to_owned()]).is_ok();
        let sub_proofs_contain_leaf = self
            .sub_proofs
            .iter()
            .any(|(_k, p)| p.contains(leaf).is_ok());
        (master_proof_contains_leaf || sub_proofs_contain_leaf)
            .then_some(())
            .ok_or(anyhow!("MKMapProof does not contain leaf {:?}", leaf))
    }
}

impl<K: MKMapKey> From<MKProof> for MKMapProof<K> {
    fn from(other: MKProof) -> Self {
        MKMapProof::new(other, BTreeMap::default())
    }
}

/// A MKMap node
#[derive(Clone)]
pub enum MKMapNode<K: MKMapKey> {
    /// A MKMap node
    Map(Rc<MKMap<K, Self>>),

    /// A MKTree node
    Tree(Rc<MKTree>),

    /// A MKTreeNode node
    TreeNode(MKTreeNode),
}

impl<K: MKMapKey> MKMapValue<K> for MKMapNode<K> {
    fn compute_root(&self) -> StdResult<MKTreeNode> {
        match self {
            MKMapNode::Map(mk_map) => mk_map.compute_root(),
            MKMapNode::Tree(merkle_tree) => merkle_tree.compute_root(),
            MKMapNode::TreeNode(merkle_tree_node) => Ok(merkle_tree_node.to_owned()),
        }
    }

    fn contains<T: Into<MKTreeNode> + Clone>(&self, leaf: &T) -> bool {
        let leaf = leaf.to_owned().into();
        match self {
            MKMapNode::Map(mk_map) => mk_map.contains(&leaf).is_some(),
            MKMapNode::Tree(merkle_tree) => merkle_tree.contains(&leaf),
            MKMapNode::TreeNode(merkle_tree_node) => *merkle_tree_node == leaf,
        }
    }

    fn compute_proof<T: Into<MKTreeNode> + Clone>(
        &self,
        leaves: &[T],
    ) -> StdResult<Option<MKMapProof<K>>> {
        match self {
            MKMapNode::Tree(ref value) => {
                let proof = value
                    .compute_proof(
                        &leaves
                            .iter()
                            .map(|leaf| leaf.to_owned().into())
                            .collect::<Vec<_>>(),
                    )
                    .with_context(|| "MKMapValue could not compute sub proof for MKTree")?;
                Ok(Some(proof.into()))
            }
            MKMapNode::Map(ref value) => {
                let proof = value
                    .compute_proof(
                        &leaves
                            .iter()
                            .map(|leaf| leaf.to_owned().into())
                            .collect::<Vec<_>>(),
                    )
                    .with_context(|| "MKMapValue could not compute sub proof for MKMap")?;
                Ok(Some(proof))
            }
            _ => Ok(None),
        }
    }
}

impl<K: MKMapKey> From<MKMap<K, MKMapNode<K>>> for MKMapNode<K> {
    fn from(other: MKMap<K, MKMapNode<K>>) -> Self {
        MKMapNode::Map(Rc::new(other))
    }
}

impl<K: MKMapKey> From<MKTree> for MKMapNode<K> {
    fn from(other: MKTree) -> Self {
        MKMapNode::Tree(Rc::new(other))
    }
}

impl<K: MKMapKey> From<MKTreeNode> for MKMapNode<K> {
    fn from(other: MKTreeNode) -> Self {
        MKMapNode::TreeNode(other)
    }
}

impl<K: MKMapKey> TryFrom<MKMapNode<K>> for MKTreeNode {
    type Error = StdError;
    fn try_from(other: MKMapNode<K>) -> Result<Self, Self::Error> {
        other.compute_root()
    }
}

#[cfg(test)]
mod tests {

    use std::collections::BTreeSet;
    use std::ops::Range;

    use crate::entities::BlockRange;

    use super::*;

    fn generate_merkle_trees(
        total_leaves: u64,
        block_range_length: u64,
    ) -> Vec<(BlockRange, MKTree)> {
        (0..total_leaves / block_range_length)
            .map(|block_range_index| {
                let block_range = BlockRange::new(
                    block_range_index * block_range_length,
                    (block_range_index + 1) * block_range_length,
                );
                let leaves = <Range<u64> as Clone>::clone(&block_range)
                    .map(|leaf_index| leaf_index.to_string())
                    .collect::<Vec<_>>();
                let merkle_tree_block_range = MKTree::new(&leaves).unwrap();

                (block_range, merkle_tree_block_range)
            })
            .collect::<Vec<_>>()
    }

    #[test]
    fn test_mk_map_should_compute_consistent_root() {
        let entries = generate_merkle_trees(100000, 100);
        let merkle_tree_node_entries = &entries
            .iter()
            .map(|(range, mktree)| {
                (
                    range.to_owned(),
                    MKMapNode::TreeNode(mktree.try_into().unwrap()),
                )
            })
            .collect::<Vec<_>>();
        let merkle_tree_full_entries = &entries
            .into_iter()
            .map(|(range, mktree)| (range.to_owned(), mktree.into()))
            .collect::<Vec<(_, MKMapNode<_>)>>();

        let mk_map_nodes = MKMap::new(merkle_tree_node_entries.as_slice()).unwrap();
        let mk_map_full = MKMap::new(merkle_tree_full_entries.as_slice()).unwrap();

        let mk_map_nodes_root = mk_map_nodes.compute_root().unwrap();
        let mk_map_full_root = mk_map_full.compute_root().unwrap();

        assert_eq!(mk_map_full_root, mk_map_nodes_root);
    }

    #[test]
    fn test_mk_map_should_accept_replacement_with_same_root_value() {
        let entries = generate_merkle_trees(1000, 10);
        let merkle_tree_entries = &entries
            .into_iter()
            .map(|(range, mktree)| (range.to_owned(), mktree.into()))
            .collect::<Vec<(_, MKMapNode<_>)>>();
        let mut mk_map = MKMap::new(merkle_tree_entries.as_slice()).unwrap();
        let block_range_replacement = BlockRange::new(0, 10);
        let same_root_value = MKMapNode::TreeNode(
            mk_map
                .get(&block_range_replacement)
                .unwrap()
                .compute_root()
                .unwrap(),
        );
        mk_map
            .insert(block_range_replacement, same_root_value)
            .unwrap();
    }

    #[test]
    fn test_mk_map_should_reject_replacement_with_different_root_value() {
        let entries = generate_merkle_trees(1000, 10);
        let merkle_tree_entries = &entries
            .into_iter()
            .map(|(range, mktree)| (range.to_owned(), mktree.into()))
            .collect::<Vec<_>>();
        let mut mk_map = MKMap::new(merkle_tree_entries.as_slice()).unwrap();
        let block_range_replacement = BlockRange::new(0, 10);
        let value_replacement: MKTreeNode = "test-123".to_string().into();
        let different_root_value = MKMapNode::TreeNode(value_replacement);
        mk_map
            .insert(block_range_replacement, different_root_value)
            .expect_err("the MKMap should reject replacement with different root value");
    }

    #[test]
    fn test_mk_map_should_reject_out_of_order_insertion() {
        let entries = generate_merkle_trees(1000, 10);
        let merkle_tree_entries = &entries
            .iter()
            .map(|(range, mktree)| {
                (
                    range.to_owned(),
                    MKMapNode::TreeNode(mktree.try_into().unwrap()),
                )
            })
            .collect::<Vec<_>>();
        let mut mk_map = MKMap::new(merkle_tree_entries.as_slice()).unwrap();
        let out_of_order_entry = (
            BlockRange::new(0, 25),
            MKMapNode::TreeNode("test-123".into()),
        );
        mk_map
            .insert(out_of_order_entry.0, out_of_order_entry.1)
            .expect_err("the MKMap should reject out of order insertion");
    }

    #[test]
    fn test_mk_map_should_list_keys_correctly() {
        let entries = generate_merkle_trees(100000, 100);
        let merkle_tree_entries = &entries
            .iter()
            .map(|(range, mktree)| {
                (
                    range.to_owned(),
                    MKMapNode::TreeNode(mktree.try_into().unwrap()),
                )
            })
            .collect::<Vec<_>>();
        let mk_map = MKMap::new(merkle_tree_entries.as_slice()).unwrap();
        let keys = mk_map
            .iter()
            .map(|(k, _v)| k.to_owned())
            .collect::<Vec<_>>();
        let expected_keys = merkle_tree_entries
            .iter()
            .map(|(k, _)| k)
            .cloned()
            .collect::<Vec<_>>();

        assert_eq!(expected_keys, keys);
    }

    #[test]
    fn test_mk_map_should_list_values_correctly() {
        let entries = generate_merkle_trees(100000, 100);
        let merkle_tree_entries = &entries
            .iter()
            .map(|(range, mktree)| {
                (
                    range.to_owned(),
                    MKMapNode::TreeNode(mktree.try_into().unwrap()),
                )
            })
            .collect::<Vec<_>>();
        let mk_map = MKMap::new(merkle_tree_entries.as_slice()).unwrap();
        let values = mk_map
            .iter()
            .map(|(_k, v)| v.to_owned())
            .collect::<Vec<_>>();
        let expected_values = merkle_tree_entries
            .iter()
            .map(|(_, v)| v)
            .cloned()
            .collect::<Vec<_>>();

        assert_eq!(
            BTreeSet::from_iter(expected_values.iter().map(|v| v.compute_root().unwrap())),
            BTreeSet::from_iter(values.iter().map(|v| v.compute_root().unwrap()))
        );
    }

    #[test]
    fn test_mk_map_should_find_value_correctly() {
        let entries = generate_merkle_trees(100000, 100);
        let mktree_node_to_certify = entries[2].1.leaves()[10].clone();
        let merkle_tree_entries = &entries
            .into_iter()
            .map(|(range, mktree)| (range.to_owned(), mktree.into()))
            .collect::<Vec<(_, MKMapNode<_>)>>();
        let mk_map_full = MKMap::new(merkle_tree_entries.as_slice()).unwrap();

        mk_map_full.contains(&mktree_node_to_certify).unwrap();
    }

    #[test]
    fn test_mk_map_should_clone_correctly() {
        let entries = generate_merkle_trees(100000, 100);
        let merkle_tree_node_entries = &entries
            .into_iter()
            .map(|(range, mktree)| (range.to_owned(), mktree.into()))
            .collect::<Vec<(_, MKMapNode<_>)>>();
        let mk_map = MKMap::new(merkle_tree_node_entries.as_slice()).unwrap();
        let mk_map_clone = mk_map.clone();

        assert_eq!(
            mk_map.compute_root().unwrap(),
            mk_map_clone.compute_root().unwrap(),
        );
    }

    #[test]
    fn test_mk_map_should_not_compute_proof_for_no_leaves() {
        let entries = generate_merkle_trees(100000, 100);
        let mktree_nodes_to_certify: &[MKTreeNode] = &[];
        let merkle_tree_node_entries = &entries
            .into_iter()
            .map(|(range, mktree)| (range.to_owned(), mktree.into()))
            .collect::<Vec<(_, MKMapNode<_>)>>();
        let mk_map_full = MKMap::new(merkle_tree_node_entries.as_slice()).unwrap();
        mk_map_full
            .compute_proof(mktree_nodes_to_certify)
            .expect_err("MKMap should not compute proof for no leaves");
    }

    #[test]
    fn test_mk_map_should_compute_and_verify_valid_proof() {
        let entries = generate_merkle_trees(100000, 100);
        let mktree_nodes_to_certify = [
            entries[0].1.leaves()[0].clone(),
            entries[2].1.leaves()[0].clone(),
            entries[2].1.leaves()[5].clone(),
            entries[3].1.leaves()[10].clone(),
        ];
        let merkle_tree_node_entries = &entries
            .into_iter()
            .map(|(range, mktree)| (range.to_owned(), mktree.into()))
            .collect::<Vec<(_, MKMapNode<_>)>>();
        let mk_map_full = MKMap::new(merkle_tree_node_entries.as_slice()).unwrap();
        let mk_map_proof = mk_map_full.compute_proof(&mktree_nodes_to_certify).unwrap();

        mk_map_proof.verify().unwrap();

        let map_proof_root = mk_map_proof.compute_root();
        let map_proof_root_expected = mk_map_full.compute_root().unwrap();
        assert_eq!(map_proof_root, map_proof_root_expected);
    }

    #[test]
    fn test_mk_map_should_compute_and_verify_valid_proof_recursively() {
        let entries = generate_merkle_trees(100000, 100);
        let mktree_nodes_to_certify = [
            entries[0].1.leaves()[0].clone(),
            entries[2].1.leaves()[5].clone(),
            entries[3].1.leaves()[10].clone(),
            entries[20].1.leaves()[0].clone(),
            entries[30].1.leaves()[0].clone(),
        ];
        let merkle_tree_node_entries = &entries
            .into_iter()
            .map(|(range, mktree)| (range.to_owned(), MKMapNode::Tree(Rc::new(mktree))))
            .collect::<Vec<_>>()
            .chunks(10)
            .map(|entries| {
                (
                    entries
                        .iter()
                        .fold(BlockRange::new(0, 0), |acc, (range, _)| {
                            acc.try_add(range).unwrap()
                        }),
                    MKMapNode::Map(Rc::new(MKMap::new(entries).unwrap())),
                )
            })
            .collect::<Vec<_>>();

        let mk_map_full = MKMap::new(merkle_tree_node_entries.as_slice()).unwrap();

        let mk_map_proof = mk_map_full.compute_proof(&mktree_nodes_to_certify).unwrap();

        mk_map_proof.verify().unwrap();

        let map_proof_root = mk_map_proof.compute_root();
        let map_proof_root_expected = mk_map_full.compute_root().unwrap();
        assert_eq!(map_proof_root, map_proof_root_expected);
    }
}
