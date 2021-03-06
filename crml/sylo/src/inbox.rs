/* Copyright 2019-2020 Centrality Investments Limited
*
* Licensed under the LGPL, Version 3.0 (the "License");
* you may not use this file except in compliance with the License.
* Unless required by applicable law or agreed to in writing, software
* distributed under the License is distributed on an "AS IS" BASIS,
* WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
* See the License for the specific language governing permissions and
* limitations under the License.
* You may obtain a copy of the License at the root of this project source code,
* or at:
*     https://centrality.ai/licenses/gplv3.txt
*     https://centrality.ai/licenses/lgplv3.txt
*/

use crate::migration;
use frame_support::{
	decl_error, decl_module, decl_storage, dispatch::DispatchResult, dispatch::Vec, ensure, weights::SimpleDispatchInfo,
};
use frame_system::ensure_signed;

const MAX_MESSAGE_LENGTH: usize = 100_000;
const MAX_DELETE_MESSAGES: usize = 10_000;

type MessageId = u32;
type Message = Vec<u8>;
type Migration<T> = migration::Module<T>;

pub trait Trait: frame_system::Trait + migration::Trait {}

decl_error! {
	pub enum Error for Module<T: Trait> {
		/// A message cannot be greater than MAX_MESSAGE_LENGTH
		MaxMessageLength,
		/// Cannot delete more than MAX_DELETE_MESSAGES at a time
		MaxDeleteMessage,
		/// Cannot assign any more ids to message due to overflow
		MessageIdOverflow,
	}
}

decl_module! {
	pub struct Module<T: Trait> for enum Call where origin: T::Origin, system = frame_system {
		type Error = Error<T>;

		/// Add a new value into storage
		///
		/// weight:
		/// O(1)
		/// 1 write
		#[weight = SimpleDispatchInfo::FixedNormal(5_000)]
		fn add_value(origin, peer_id: T::AccountId, value: Message) -> DispatchResult {
			ensure_signed(origin)?;
			ensure!(value.len() <= MAX_MESSAGE_LENGTH, Error::<T>::MaxMessageLength);
			Self::add(peer_id, value)
		}

		/// Delete a value from storage
		///
		/// weight:
		/// O(n) where n is number of values in the storage
		/// 1 write
		#[weight = SimpleDispatchInfo::FixedNormal(10_000)]
		fn delete_values(origin, value_ids: Vec<MessageId>) -> DispatchResult {
			let user_id = ensure_signed(origin)?;
			ensure!(value_ids.len() <= MAX_DELETE_MESSAGES, Error::<T>::MaxDeleteMessage);
			Self::delete(user_id, value_ids)
		}

		#[weight = SimpleDispatchInfo::FixedOperational(0)]
		fn migrate_inbox(origin, user_id: T::AccountId, next_index: MessageId, new_messages: Vec<(MessageId, Message)>) -> DispatchResult {
			<Migration<T>>::ensure_sylo_migrator(origin)?;

			let mut existing_messages = <Values<T>>::get(&user_id);
			let (existing_indexes, _): (Vec<MessageId>, Vec<_>) = existing_messages.clone().into_iter().unzip();

			// For repeatability, we update the existing messages that are assumed to be migrated already.
			for (new_index, new_message) in new_messages {
				if !existing_indexes.contains(&new_index) {
					existing_messages.push((new_index, new_message));
				}
			}

			<Values<T>>::insert(&user_id, existing_messages);
			<NextIndexes<T>>::insert(&user_id, next_index);
			Ok(())
		}
	}
}

decl_storage! {
	trait Store for Module<T: Trait> as SyloInbox {
		NextIndexes: map hasher(blake2_128_concat) T::AccountId => MessageId;
		Values get(values): map hasher(blake2_128_concat) T::AccountId => Vec<(MessageId, Message)>;
	}
}

impl<T: Trait> Module<T> {
	pub fn inbox(who: T::AccountId) -> Vec<Message> {
		<Values<T>>::get(who).into_iter().map(|(_, value)| value).collect()
	}

	pub fn add(peer_id: T::AccountId, value: Message) -> DispatchResult {
		// Get required data
		let next_index = <NextIndexes<T>>::get(&peer_id);
		ensure!(next_index != u32::max_value(), Error::<T>::MessageIdOverflow);

		// Store data
		let mut values = <Values<T>>::get(&peer_id);
		values.push((next_index, value));
		<Values<T>>::insert(peer_id.clone(), values);

		// Update next_index
		<NextIndexes<T>>::insert(&peer_id, next_index + 1);

		Ok(())
	}

	pub fn delete(user_id: T::AccountId, value_ids: Vec<MessageId>) -> DispatchResult {
		let mut values = <Values<T>>::get(&user_id);
		for id in value_ids {
			// Remove value from storage
			if let Some(index) = values.iter().position(|(x, _)| *x == id) {
				values.remove(index);
			}
		}
		<Values<T>>::insert(user_id, values);
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mock::{ExtBuilder, Origin, Test};
	use frame_support::assert_ok;
	use sp_core::H256;

	type Inbox = Module<Test>;
	type Migration = migration::Module<Test>;

	#[test]
	fn it_works_adding_values_to_an_inbox() {
		ExtBuilder::default().build().execute_with(|| {
			// Add a value to an empty inbox
			assert_ok!(Inbox::add_value(
				Origin::signed(H256::from_low_u64_be(1)),
				H256::from_low_u64_be(2),
				b"hello, world".to_vec()
			));
			assert_eq!(Inbox::inbox(H256::from_low_u64_be(2)), vec![b"hello, world".to_vec()]);

			// Add another value
			assert_ok!(Inbox::add_value(
				Origin::signed(H256::from_low_u64_be(1)),
				H256::from_low_u64_be(2),
				b"sylo".to_vec()
			));
			assert_eq!(
				Inbox::inbox(H256::from_low_u64_be(2)),
				vec![b"hello, world".to_vec(), b"sylo".to_vec()]
			);
		});
	}

	#[test]
	fn it_works_removing_values_from_an_inbox() {
		ExtBuilder::default().build().execute_with(|| {
			// Add values to an empty inbox
			assert_ok!(Inbox::add_value(
				Origin::signed(H256::from_low_u64_be(1)),
				H256::from_low_u64_be(2),
				b"hello, world".to_vec()
			));
			assert_ok!(Inbox::add_value(
				Origin::signed(H256::from_low_u64_be(1)),
				H256::from_low_u64_be(2),
				b"sylo".to_vec()
			));
			assert_ok!(Inbox::add_value(
				Origin::signed(H256::from_low_u64_be(1)),
				H256::from_low_u64_be(2),
				b"foo".to_vec()
			));
			assert_ok!(Inbox::add_value(
				Origin::signed(H256::from_low_u64_be(1)),
				H256::from_low_u64_be(2),
				b"bar".to_vec()
			));

			// Remove a single value
			assert_ok!(Inbox::delete_values(Origin::signed(H256::from_low_u64_be(2)), vec![0]));
			assert_eq!(
				Inbox::inbox(H256::from_low_u64_be(2)),
				vec![b"sylo".to_vec(), b"foo".to_vec(), b"bar".to_vec()]
			);

			assert_ok!(Inbox::delete_values(
				Origin::signed(H256::from_low_u64_be(2)),
				vec![2, 3]
			));
			assert_eq!(Inbox::inbox(H256::from_low_u64_be(2)), vec![b"sylo".to_vec()]);
		});
	}

	#[test]
	fn it_works_removing_values_from_an_empty_inbox() {
		ExtBuilder::default().build().execute_with(|| {
			// Remove a value that doesn't exist
			assert_ok!(Inbox::delete_values(Origin::signed(H256::from_low_u64_be(2)), vec![0]));
		});
	}

	#[test]
	fn migrate_inbox_works() {
		ExtBuilder::default().build().execute_with(|| {
			let user_id = H256::from_low_u64_be(1);
			let messages = vec![
				(0, b"test0".to_vec()),
				(1, b"test1".to_vec()),
				(2, b"test2".to_vec()),
				(3, b"test3".to_vec()),
			];
			let next_index = 7357;
			let migration_account = H256::from_low_u64_be(2);

			assert_ok!(Migration::set_migrator_account(Origin::ROOT, migration_account));
			assert_ok!(Inbox::migrate_inbox(
				Origin::signed(migration_account),
				user_id.clone(),
				next_index.clone(),
				messages.clone()
			));

			assert_eq!(Inbox::values(user_id), messages);
			assert_eq!(NextIndexes::<Test>::get(user_id), next_index);
		});
	}

	#[test]
	fn migrate_inbox_works_with_existing_data() {
		ExtBuilder::default().build().execute_with(|| {
			let user_id = H256::from_low_u64_be(1);
			let next_index = 7357;
			let existing_messages = vec![
				b"test0".to_vec(),
				b"test1".to_vec(),
				b"test2".to_vec(),
				b"test3".to_vec(),
			];
			for message in existing_messages {
				assert_ok!(Inbox::add(user_id, message));
			}

			let migration_account = H256::from_low_u64_be(2);
			let new_messages = vec![
				(0, b"different_test_message_0".to_vec()),
				(1, b"different_test_message_1".to_vec()),
				(2, b"different_test_message_2".to_vec()),
				(3, b"test3".to_vec()),
				(4, b"test4".to_vec()),
				(5, b"test4".to_vec()),
			];

			assert_ok!(Migration::set_migrator_account(Origin::ROOT, migration_account));
			assert_ok!(Inbox::migrate_inbox(
				Origin::signed(migration_account),
				user_id.clone(),
				next_index.clone(),
				new_messages.clone()
			));

			let current_messages = vec![
				(0, b"test0".to_vec()), // existing data untouched
				(1, b"test1".to_vec()), // existing data untouched
				(2, b"test2".to_vec()), // existing data untouched
				(3, b"test3".to_vec()),
				(4, b"test4".to_vec()),
				(5, b"test4".to_vec()),
			];
			assert_eq!(Inbox::values(user_id), current_messages);
			assert_eq!(NextIndexes::<Test>::get(user_id), next_index);
		});
	}
}
