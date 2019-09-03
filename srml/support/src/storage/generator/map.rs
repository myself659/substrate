// Copyright 2019 Parity Technologies (UK) Ltd.
// This file is part of Substrate.

// Substrate is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Substrate is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Substrate.  If not, see <http://www.gnu.org/licenses/>.

#[cfg(not(feature = "std"))]
use rstd::prelude::*;
use rstd::iter::FromIterator;
use rstd::borrow::Borrow;
use codec::{Codec, Encode, EncodeLike, EncodeAppend};
use crate::{storage::{self, unhashed, hashed::StorageHasher}, traits::Len};

/// Generator for `StorageMap` used by `decl_storage`.
///
/// For each key value is stored at `Hasher(prefix ++ key)`.
pub trait StorageMap<K: Codec, V: Codec> {
	/// The type that get/take returns.
	type Query;

	/// Hasher used to insert into storage.
	type Hasher: StorageHasher;

	/// Prefix used to prepend each key.
	fn prefix() -> &'static [u8];

	/// Convert an optional value retrieved from storage to the type queried.
	fn from_optional_value_to_query(v: Option<V>) -> Self::Query;

	/// Convert a query to an optional value into storage.
	fn from_query_to_optional_value(v: Self::Query) -> Option<V>;

	/// Generate the full key used in top storage.
	fn storage_map_final_key<KeyArg>(key: &KeyArg) -> <Self::Hasher as StorageHasher>::Output
	where
		KeyArg: EncodeLike<K>,
	{
		let mut final_key = Self::prefix().to_vec();
		key.encode_to(&mut final_key);
		Self::Hasher::hash(&final_key)
	}
}

impl<K: Codec, V: Codec, G: StorageMap<K, V>> storage::StorageMap<K, V> for G {
	type Query = G::Query;

	fn hashed_key_for<KeyArg: EncodeLike<K>>(key: &KeyArg) -> Vec<u8> {
		Self::storage_map_final_key(key).as_ref().to_vec()
	}

	fn swap<KeyArg1: EncodeLike<K>, KeyArg2: EncodeLike<K>>(key1: &KeyArg1, key2: &KeyArg2) {
		let k1 = Self::storage_map_final_key(key1);
		let k2 = Self::storage_map_final_key(key2);

		let v1 = unhashed::get_raw(k1.as_ref());
		if let Some(val) = unhashed::get_raw(k2.as_ref()) {
			unhashed::put_raw(k1.as_ref(), &val);
		} else {
			unhashed::kill(k1.as_ref())
		}
		if let Some(val) = v1 {
			unhashed::put_raw(k2.as_ref(), &val);
		} else {
			unhashed::kill(k2.as_ref())
		}
	}

	fn exists<KeyArg: EncodeLike<K>>(key: &KeyArg) -> bool {
		unhashed::exists(Self::storage_map_final_key(key).as_ref())
	}

	fn get<KeyArg: EncodeLike<K>>(key: &KeyArg) -> Self::Query {
		G::from_optional_value_to_query(unhashed::get(Self::storage_map_final_key(key).as_ref()))
	}

	fn insert<KeyArg: EncodeLike<K>, ValArg: EncodeLike<V>>(key: &KeyArg, val: &ValArg) {
		unhashed::put(Self::storage_map_final_key(key).as_ref(), &val)
	}

	fn remove<KeyArg: EncodeLike<K>>(key: &KeyArg) {
		unhashed::kill(Self::storage_map_final_key(key).as_ref())
	}

	fn mutate<KeyArg: EncodeLike<K>, R, F: FnOnce(&mut Self::Query) -> R>(key: &KeyArg, f: F) -> R {
		let mut val = G::get(key);

		let ret = f(&mut val);
		match G::from_query_to_optional_value(val) {
			Some(ref val) => G::insert(key, &val),
			None => G::remove(key),
		}
		ret
	}

	fn take<KeyArg: EncodeLike<K>>(key: &KeyArg) -> Self::Query {
		let key = Self::storage_map_final_key(key);
		let value = unhashed::take(key.as_ref());
		G::from_optional_value_to_query(value)
	}

	fn append<'a, Iter, Item, EncodeLikeItem, KeyArg>(key: &KeyArg, items: Iter)
		-> Result<(), &'static str>
	where
		KeyArg: EncodeLike<K>,
		Item: Encode,
		EncodeLikeItem: 'a + EncodeLike<Item>,
		V: EncodeAppend<Item=Item>,
		Iter: IntoIterator<Item=&'a EncodeLikeItem>,
		Iter::IntoIter: ExactSizeIterator,
	{
		let key = Self::storage_map_final_key(key);
		let encoded_value = unhashed::get_raw(key.as_ref())
			.unwrap_or_else(|| {
				match G::from_query_to_optional_value(G::from_optional_value_to_query(None)) {
					Some(value) => value.encode(),
					None => vec![],
				}
			});

		let new_val = V::append_or_new(
			encoded_value,
			items,
		).map_err(|_| "Could not append given item")?;
		unhashed::put_raw(key.as_ref(), &new_val);
		Ok(())
	}

	fn append_or_put<'a, Iter, Item, EncodeLikeItem, EncodeLikeV, KeyArg>(key: &KeyArg, items: Iter)
	where
		KeyArg: EncodeLike<K>,
		Item: Encode,
		EncodeLikeV: EncodeLike<V> + FromIterator<EncodeLikeItem>,
		EncodeLikeItem: 'a + EncodeLike<Item> + Clone,
		V: EncodeAppend<Item=Item>,
		Iter: IntoIterator<Item=&'a EncodeLikeItem> + Clone,
		Iter::IntoIter: ExactSizeIterator,
	{
		Self::append(key.borrow(), items.clone()).unwrap_or_else(|_| {
			let put: EncodeLikeV = items.into_iter().cloned().collect();
			Self::insert(key, &put)
		});
	}

	fn decode_len<KeyArg: EncodeLike<K>>(key: &KeyArg) -> Result<usize, &'static str>
		where V: codec::DecodeLength + Len
	{
		let key = Self::storage_map_final_key(key);
		if let Some(v) = unhashed::get_raw(key.as_ref()) {
			<V as codec::DecodeLength>::len(&v).map_err(|e| e.what())
		} else {
			let len = G::from_query_to_optional_value(G::from_optional_value_to_query(None))
				.map(|v| v.len())
				.unwrap_or(0);

			Ok(len)
		}
	}
}