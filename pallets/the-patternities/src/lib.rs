#![cfg_attr(not(feature = "std"), no_std)]

/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// <https://docs.substrate.io/v3/runtime/frame>
pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use enocoro128v2::Enocoro128;
	use frame_support::pallet_prelude::*;
	use frame_support::{
		sp_runtime::traits::Hash,
		traits::{tokens::ExistenceRequirement, Currency},
		transactional,
	};
	use frame_system::pallet_prelude::*;
	use nanorand::{Rng, WyRand};
	use sp_std::vec::Vec;

	type AccountOf<T> = <T as frame_system::Config>::AccountId;
	type BalanceOf<T> =
		<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	#[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
	#[scale_info(skip_type_params(T))]
	#[codec(mel_bound())]
	pub struct PatternSeed<T: Config> {
		pub key: [u8; 16],
		pub iv: [u8; 8],
		pub cipher: BoundedVec<u8, T::MaxByteCipher>,
		pub price: Option<BalanceOf<T>>,
		pub owner: AccountOf<T>,
	}

	#[pallet::config]
	pub trait Config: frame_system::Config {
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		type Currency: Currency<Self::AccountId>;
		#[pallet::constant]
		type MaxPatternityOwned: Get<u32>;
		#[pallet::constant]
		type MaxByteCipher: Get<u32>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	#[pallet::storage]
	#[pallet::getter(fn patternity)]
	pub(super) type Patternity<T: Config> = StorageMap<_, Twox64Concat, T::Hash, PatternSeed<T>>;

	#[pallet::storage]
	#[pallet::getter(fn patternity_owned)]
	pub(super) type PatternityOwned<T: Config> = StorageMap<
		_,
		Twox64Concat,
		T::AccountId,
		BoundedVec<T::Hash, T::MaxPatternityOwned>,
		ValueQuery,
	>;

	#[pallet::storage]
	#[pallet::getter(fn patternity_cnt)]
	pub(super) type PatternityCnt<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub the_patternities: Vec<(T::AccountId, Option<BalanceOf<T>>)>,
	}

	#[cfg(feature = "std")]
	impl<T: Config> Default for GenesisConfig<T> {
		fn default() -> GenesisConfig<T> {
			GenesisConfig { the_patternities: vec![] }
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> GenesisBuild<T> for GenesisConfig<T> {
		fn build(&self) {
			for (owner, price) in &self.the_patternities {
				let _ = <Pallet<T>>::mint(owner, price.clone());
			}
		}
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		Created(T::AccountId, T::Hash),
		PriceSet(T::AccountId, T::Hash, Option<BalanceOf<T>>),
		Transferred(T::AccountId, T::AccountId, T::Hash),
		Bought(T::AccountId, T::AccountId, T::Hash, BalanceOf<T>),
	}

	#[pallet::error]
	pub enum Error<T> {
		PatternityOwned,
		PatternCntOverflow,
		ExceedMaxPatternityOwned,
		BuyerIsPatternOwner,
		TransferToSelf,
		PatternNotExist,
		NotPatternOwner,
		PatternNotForSale,
		PatternBidPriceTooLow,
		NotEnoughBalance,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(100)]
		pub fn create_pattern(origin: OriginFor<T>, price: Option<BalanceOf<T>>) -> DispatchResult {
			let sender = ensure_signed(origin)?;

			let pattern_obj = Self::mint(&sender, price);

			let pattern_id = T::Hashing::hash_of(&pattern_obj);

			<PatternityOwned<T>>::try_mutate(&sender, |patt_vec| patt_vec.try_push(pattern_id))
				.map_err(|_| <Error<T>>::ExceedMaxPatternityOwned)?;

			<Patternity<T>>::insert(pattern_id, pattern_obj);

			let new_cnt =
				Self::patternity_cnt().checked_add(1).ok_or(<Error<T>>::PatternCntOverflow)?;

			<PatternityCnt<T>>::put(new_cnt);

			Self::deposit_event(Event::Created(sender, pattern_id));
			Ok(())
		}

		#[pallet::weight(100)]
		pub fn set_price(
			origin: OriginFor<T>,
			pattern_id: T::Hash,
			new_price: Option<BalanceOf<T>>,
		) -> DispatchResult {
			let sender = ensure_signed(origin)?;

			ensure!(Self::is_pattern_owner(&pattern_id, &sender)?, <Error<T>>::NotPatternOwner);

			let mut pattern = Self::patternity(&pattern_id).ok_or(<Error<T>>::PatternNotExist)?;

			pattern.price = new_price.clone();
			<Patternity<T>>::insert(&pattern_id, pattern);

			// Deposit a "PriceSet" event.
			Self::deposit_event(Event::PriceSet(sender, pattern_id, new_price));

			Ok(())
		}

		#[pallet::weight(100)]
		pub fn transfer(
			origin: OriginFor<T>,
			to: T::AccountId,
			pattern_id: T::Hash,
		) -> DispatchResult {
			let from = ensure_signed(origin)?;

			ensure!(Self::is_pattern_owner(&pattern_id, &from)?, <Error<T>>::NotPatternOwner);

			ensure!(from != to, <Error<T>>::TransferToSelf);

			let to_owned = <PatternityOwned<T>>::get(&to);
			ensure!(
				(to_owned.len() as u32) < T::MaxPatternityOwned::get(),
				<Error<T>>::ExceedMaxPatternityOwned
			);

			Self::transfer_pattern_to(&pattern_id, &to)?;

			Self::deposit_event(Event::Transferred(from, to, pattern_id));

			Ok(())
		}

		#[transactional]
		#[pallet::weight(100)]
		pub fn buy_pattern(
			origin: OriginFor<T>,
			pattern_id: T::Hash,
			bid_price: BalanceOf<T>,
		) -> DispatchResult {
			let buyer = ensure_signed(origin)?;

			let pattern = Self::patternity(&pattern_id).ok_or(<Error<T>>::PatternNotExist)?;
			ensure!(pattern.owner != buyer, <Error<T>>::BuyerIsPatternOwner);

			if let Some(ask_price) = pattern.price {
				ensure!(ask_price <= bid_price, <Error<T>>::PatternBidPriceTooLow);
			} else {
				Err(<Error<T>>::PatternNotForSale)?;
			}

			ensure!(T::Currency::free_balance(&buyer) >= bid_price, <Error<T>>::NotEnoughBalance);

			let to_owned = <PatternityOwned<T>>::get(&buyer);
			ensure!(
				(to_owned.len() as u32) < T::MaxPatternityOwned::get(),
				<Error<T>>::ExceedMaxPatternityOwned
			);

			let seller = pattern.owner.clone();

			T::Currency::transfer(&buyer, &seller, bid_price, ExistenceRequirement::KeepAlive)?;

			Self::transfer_pattern_to(&pattern_id, &buyer)?;

			Self::deposit_event(Event::Bought(buyer, seller, pattern_id, bid_price));

			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		fn generate_random_key() -> [u8; 16] {
			let mut rng = WyRand::new();

			let key: [u8; 16] = [
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
			];

			let iv: [u8; 8] = [
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
			];

			let mut e128 = Enocoro128::new(&key, &iv);

			let ret: [u8; 16] = [
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
			];

			ret
		}

		fn generate_random_iv() -> [u8; 8] {
			let mut rng = WyRand::new();

			let key: [u8; 16] = [
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
			];

			let iv: [u8; 8] = [
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
				rng.generate::<u8>(),
			];

			let mut e128 = Enocoro128::new(&key, &iv);

			let ret: [u8; 8] = [
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
				e128.rand_u8(),
			];

			ret
		}

		fn cipher_code(key: [u8; 16], iv: [u8; 8]) -> Vec<u8> {
			let text_base = "
					use tiny_skia::*;
					use rand::prelude::*;
					use rand_chacha::ChaCha20Rng;
					use std::env;

					fn main() {
						let args: Vec<String> = env::args().collect();

						let seed_arg = &args[1];

						let seed = seed_arg.parse::<u64>().expect(\"Error parsing seed\");

						let triangle = crate_triangle(seed);

						let mut paint = Paint::default();
						paint.anti_alias = true;
						paint.shader = Pattern::new(
							triangle.as_ref(),
							SpreadMode::Repeat,
							FilterQuality::Bicubic,
							1.0,
							Transform::from_row(1.5, -0.4, 0.0, -0.8, 5.0, 1.0),
						);

						let path = PathBuilder::from_circle(200.0, 200.0, 180.0).unwrap();

						let mut pixmap = Pixmap::new(400, 400).unwrap();
						pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
						pixmap.save_png(format!(\"{}-pattern.png\", seed)).unwrap();
					}

					fn crate_triangle(hash: u64) -> Pixmap {
						let mut rng = ChaCha20Rng::seed_from_u64(hash);

						let r: u8 = rng.gen_range(0..255);
						let g: u8 = rng.gen_range(0..255);
						let b: u8 = rng.gen_range(0..255);


						let mut paint = Paint::default();
						paint.set_color_rgba8(r, g, b, 255);
						paint.anti_alias = true;

						let start: u8 = rng.gen_range(0..10);
						let end: u8 = rng.gen_range(10..25);

						let start_second: u8 = rng.gen_range(25..50);
						let end_second: u8 = rng.gen_range(50..75);

						let start_third: u8 = rng.gen_range(10..20);
						let end_third: u8 = rng.gen_range(0..20);

						let mut pb = PathBuilder::new();
						pb.move_to(start as f32, end as f32);
						pb.line_to(start_second as f32, end_second as f32);
						pb.line_to(start_third as f32, end_third as f32);

						let path = pb.finish().unwrap();

						let mut pixmap = Pixmap::new(20, 20).unwrap();
						pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
						pixmap
					}";

			let mut msg: Vec<u8> = text_base.as_bytes().to_vec();

			Enocoro128::apply_keystream_static(&key, &iv, &mut msg);

			msg
		}

		fn mint(account_id: &T::AccountId, price: Option<BalanceOf<T>>) -> PatternSeed<T> {
			let key = Self::generate_random_key();
			let iv = Self::generate_random_iv();

			let cipher = Self::cipher_code(key, iv);

			let cipher_bv: BoundedVec<u8, T::MaxByteCipher> =
				BoundedVec::try_from(cipher.to_vec()).unwrap();

			PatternSeed::<T> { key, iv, cipher: cipher_bv, owner: account_id.clone(), price }
		}

		pub fn is_pattern_owner(
			pattern_id: &T::Hash,
			acct: &T::AccountId,
		) -> Result<bool, Error<T>> {
			match Self::patternity(pattern_id) {
				Some(pattern) => Ok(pattern.owner == *acct),
				None => Err(<Error<T>>::PatternNotExist),
			}
		}

		#[transactional]
		pub fn transfer_pattern_to(kitty_id: &T::Hash, to: &T::AccountId) -> Result<(), Error<T>> {
			let mut kitty = Self::patternity(&kitty_id).ok_or(<Error<T>>::PatternNotExist)?;

			let prev_owner = kitty.owner.clone();

			// Remove `kitty_id` from the KittyOwned vector of `prev_kitty_owner`
			<PatternityOwned<T>>::try_mutate(&prev_owner, |owned| {
				if let Some(ind) = owned.iter().position(|&id| id == *kitty_id) {
					owned.swap_remove(ind);
					return Ok(());
				}
				Err(())
			})
			.map_err(|_| <Error<T>>::PatternityOwned)?;

			// Update the kitty owner
			kitty.owner = to.clone();
			// Reset the ask price so the kitty is not for sale until `set_price()` is called
			// by the current owner.
			kitty.price = None;

			<Patternity<T>>::insert(kitty_id, kitty);

			<PatternityOwned<T>>::try_mutate(to, |vec| vec.try_push(*kitty_id))
				.map_err(|_| <Error<T>>::ExceedMaxPatternityOwned)?;

			Ok(())
		}
	}
}
