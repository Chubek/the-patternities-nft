#![cfg_attr(not(feature = "std"), no_std)]

/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// <https://docs.substrate.io/v3/runtime/frame>
pub use pallet::*;




#[frame_support::pallet]
pub mod pallet {
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;
	use frame_support::{
		sp_runtime::traits::Hash,
		traits::{ Currency, tokens::ExistenceRequirement },
		transactional
	};
	use enocoro128v2::Enocoro128;
	use nanorand::{Rng, WyRand};


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
		pub owner: AccountOf<T>
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
	pub(super) type PatternityOwned<T: Config> =
		StorageMap<_, Twox64Concat, T::AccountId, BoundedVec<T::Hash, T::MaxPatternityOwned>, ValueQuery>;

	
	#[pallet::storage]
	#[pallet::getter(fn patternity_cnt)]
	pub(super) type PatternityCnt<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub patternity_genesis: Vec<(T::AccountId, Option<BalanceOf<T>>)>,
	}



	#[cfg(feature = "std")]
	impl<T: Config> Default for GenesisConfig<T> {
		fn default() -> GenesisConfig<T> {
			GenesisConfig { patternity_genesis: vec![] }
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> GenesisBuild<T> for GenesisConfig<T> {
		fn build(&self) {
			for (owner, price) in &self.patternity_genesis {
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
		NotEnoughBalance
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {	
		#[pallet::weight(100)]
		pub fn create_pattern(origin: OriginFor<T>, price: Option<BalanceOf<T>>) -> DispatchResult {
			let sender = ensure_signed(origin)?;

			let pattern_obj = Self::mint(&sender, price);

			let pattern_id = T::Hashing::hash_of(&pattern_obj);


			<PatternityOwned<T>>::try_mutate(&sender, |patt_vec| {
				patt_vec.try_push(pattern_id)
			}).map_err(|_| <Error<T>>::ExceedMaxPatternityOwned)?;

			<Patternity<T>>::insert(pattern_id, pattern_obj);

			let new_cnt = Self::patternity_cnt().checked_add(1)
				.ok_or(<Error<T>>::PatternCntOverflow)?;

			<PatternityCnt<T>>::put(new_cnt);

			Self::deposit_event(Event::Created(sender, pattern_id));
			Ok(())
		}


		#[pallet::weight(100)]
		pub fn set_price(
			origin: OriginFor<T>,
			pattern_id: T::Hash,
			new_price: Option<BalanceOf<T>>
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
			pattern_id: T::Hash
		) -> DispatchResult {
			let from = ensure_signed(origin)?;

			ensure!(Self::is_pattern_owner(&pattern_id, &from)?, <Error<T>>::NotPatternOwner);

			ensure!(from != to, <Error<T>>::TransferToSelf);

			let to_owned = <PatternityOwned<T>>::get(&to);
			ensure!((to_owned.len() as u32) < T::MaxPatternityOwned::get(), <Error<T>>::ExceedMaxPatternityOwned);

			Self::transfer_pattern_to(&pattern_id, &to)?;

			Self::deposit_event(Event::Transferred(from, to, pattern_id));

			Ok(())
		}

		#[transactional]
		#[pallet::weight(100)]
		pub fn buy_pattern(
			origin: OriginFor<T>,
			pattern_id: T::Hash,
			bid_price: BalanceOf<T>
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
			ensure!((to_owned.len() as u32) < T::MaxPatternityOwned::get(), <Error<T>>::ExceedMaxPatternityOwned);

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
    			rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), 
				rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(),
    			rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), 
				rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(),
			];

			let iv: [u8; 8] = [rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(),
			rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>()];
		
			let mut e128 = Enocoro128::new(&key, &iv);
			
			let ret: [u8; 16] = [e128.rand_u8(), e128.rand_u8(), e128.rand_u8(),
					e128.rand_u8(), e128.rand_u8(), e128.rand_u8(),
					e128.rand_u8(), e128.rand_u8(), e128.rand_u8(),
					e128.rand_u8(), e128.rand_u8(), e128.rand_u8(),
					e128.rand_u8(),e128.rand_u8(),e128.rand_u8(),e128.rand_u8()];

			ret
		
		}

		fn generate_random_iv() -> [u8; 8] {
			let mut rng = WyRand::new();
			
			let key: [u8; 16] = [
    			rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), 
				rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(),
    			rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), 
				rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(),
			];

			let iv: [u8; 8] = [rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(),
			rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>(), rng.generate::<u8>()];
		
			let mut e128 = Enocoro128::new(&key, &iv);
			
			let ret: [u8; 8] = [e128.rand_u8(), e128.rand_u8(), e128.rand_u8(),
					e128.rand_u8(), e128.rand_u8(), e128.rand_u8(),
					e128.rand_u8(), e128.rand_u8()];

			ret
		
		}

		fn cipher_code(key: [u8; 16], iv: [u8; 8]) -> [u8; 2000] {
			let text_base: [u8; 2000] = [10, 9, 9, 9, 9, 9, 117, 115, 101, 32, 116, 105, 110, 121, 95, 
			115, 107, 105, 97, 58, 58, 42, 59, 10, 9, 9, 9, 9, 9, 117, 115, 101, 32, 114, 97, 110, 100, 58, 58, 
			112, 114, 101, 108, 117, 100, 101, 58, 58, 42, 59, 10, 9, 9, 9, 9, 9, 117, 115, 101, 32, 114, 97, 110, 
			100, 95, 99, 104, 97, 99, 104, 97, 58, 58, 67, 104, 97, 67, 104, 97, 50, 48, 82, 110, 103, 59, 10, 9, 9, 9,
			 9, 9, 117, 115, 101, 32, 115, 116, 100, 58, 58, 101, 110, 118, 59, 10, 9, 9, 9, 9, 9, 10, 9, 9, 9, 9, 9, 102, 
			 110, 32, 109, 97, 105, 110, 40, 41, 32, 123, 10, 9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 97, 114, 103, 115, 58, 32,
			  86, 101, 99, 60, 83, 116, 114, 105, 110, 103, 62, 32, 61, 32, 101, 110, 118, 58, 58, 97, 114, 103, 115, 40, 41, 
			  46, 99, 111, 108, 108, 101, 99, 116, 40, 41, 59, 10, 9, 9, 9, 9, 9, 10, 9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 115, 
			  101, 101, 100, 95, 97, 114, 103, 32, 61, 32, 38, 97, 114, 103, 115, 91, 49, 93, 59, 10, 9, 9, 9, 9, 9, 10, 9, 9, 
			  9, 9, 9, 9, 108, 101, 116, 32, 115, 101, 101, 100, 32, 61, 32, 115, 101, 101, 100, 95, 97, 114, 103, 46, 112, 97, 
			  114, 115, 101, 58, 58, 60, 117, 54, 52, 62, 40, 41, 46, 101, 120, 112, 101, 99, 116, 40, 34, 69, 114, 114, 111, 
			  114, 32, 112, 97, 114, 115, 105, 110, 103, 32, 115, 101, 101, 100, 33, 33, 33, 34, 41, 59, 10, 9, 9, 9, 9, 9, 10,
			   9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 116, 114, 105, 97, 110, 103, 108, 101, 32, 61, 32, 99, 114, 97, 116, 101, 
			   95, 116, 114, 105, 97, 110, 103, 108, 101, 40, 115, 101, 101, 100, 41, 59, 10, 9, 9, 9, 9, 9, 10, 9, 9, 9, 9, 9, 
			   9, 108, 101, 116, 32, 109, 117, 116, 32, 112, 97, 105, 110, 116, 32, 61, 32, 80, 97, 105, 110, 116, 58, 58, 100, 
			   101, 102, 97, 117, 108, 116, 40, 41, 59, 10, 9, 9, 9, 9, 9, 9, 112, 97, 105, 110, 116, 46, 97, 110, 116, 105, 95,
			    97, 108, 105, 97, 115, 32, 61, 32, 116, 114, 117, 101, 59, 10, 9, 9, 9, 9, 9, 9, 112, 97, 105, 110, 116, 46, 115, 
			    104, 97, 100, 101, 114, 32, 61, 32, 80, 97, 116, 116, 101, 114, 110, 58, 58, 110, 101, 119, 40, 10, 9, 9, 9, 9, 
			    9, 9, 9, 116, 114, 105, 97, 110, 103, 108, 101, 46, 97, 115, 95, 114, 101, 102, 40, 41, 44, 10, 9, 9, 9, 9, 9, 
			    9, 9, 83, 112, 114, 101, 97, 100, 77, 111, 100, 101, 58, 58, 82, 101, 112, 101, 97, 116, 44, 10, 9, 9, 9, 9, 
			    9, 9, 9, 70, 105, 108, 116, 101, 114, 81, 117, 97, 108, 105, 116, 121, 58, 58, 66, 105, 99, 117, 98, 105, 9,
			    9, 44, 10, 9, 9, 9, 9, 9, 9, 9, 49, 46, 48, 44, 10, 9, 9, 9, 9, 9, 9, 9, 84, 114, 97, 110, 115, 102, 111,
			     114, 109, 58, 58, 102, 114, 111, 109, 95, 114, 111, 119, 40, 49, 46, 53, 44, 32, 45, 48, 46, 52, 44, 32,
			      48, 46, 48, 44, 32, 45, 48, 46, 56, 44, 32, 53, 46, 48, 44, 32, 49, 46, 48, 41, 44, 10, 9, 9, 9, 9, 9,
			       9, 41, 59, 10, 9, 9, 9, 9, 9, 10, 9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 112, 97, 116, 104, 32, 61, 32, 
			       80, 97, 116, 104, 66, 117, 105, 108, 100, 101, 114, 58, 58, 102, 114, 111, 109, 95, 99, 105, 114, 99, 
			       108, 101, 40, 50, 48, 48, 46, 48, 44, 32, 50, 48, 48, 46, 48, 44, 32, 49, 56, 48, 46, 48, 41, 46, 117, 
			       110, 119, 114, 97, 112, 40, 41, 59, 10, 9, 9, 9, 9, 9, 10, 9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 109,
			        117, 116, 32, 112, 105, 120, 109, 97, 112, 32, 61, 32, 80, 105, 120, 109, 97, 112, 58, 58, 110, 101,
			         119, 40, 52, 48, 48, 44, 32, 52, 48, 48, 41, 46, 117, 110, 119, 114, 97, 112, 40, 41, 59, 10, 9, 
			         9, 9, 9, 9, 9, 112, 105, 120, 109, 97, 112, 46, 102, 105, 108, 108, 95, 112, 97, 116, 104, 40, 38,
			          112, 97, 116, 104, 44, 32, 38, 112, 97, 105, 110, 116, 44, 32, 70, 105, 108, 108, 82, 117, 108, 
			          101, 58, 58, 87, 105, 110, 100, 105, 110, 103, 44, 32, 84, 114, 97, 110, 115, 102, 111, 114,
			           109, 58, 58, 105, 100, 101, 110, 116, 105, 116, 121, 40, 41, 44, 32, 78, 111, 110, 101, 41, 
			           59, 10, 9, 9, 9, 9, 9, 9, 112, 105, 120, 109, 97, 112, 46, 115, 97, 118, 101, 95, 112, 110, 
			           103, 40, 102, 111, 114, 109, 97, 116, 33, 40, 34, 123, 125, 45, 112, 97, 116, 116, 101, 114, 
			           110, 46, 112, 110, 103, 34, 44, 32, 115, 101, 101, 100, 41, 41, 46, 117, 110, 119, 114, 97, 
			           112, 40, 41, 59, 10, 9, 9, 9, 9, 9, 125, 10, 9, 9, 9, 9, 9, 10, 9, 9, 9, 9, 9, 102, 110, 32, 
			           99, 114, 97, 116, 101, 95, 116, 114, 105, 97, 110, 103, 108, 101, 40, 104, 97, 115, 104, 58, 
			           32, 117, 54, 52, 41, 32, 45, 62, 32, 80, 105, 120, 109, 97, 112, 32, 123, 10, 9, 9, 9, 9, 9, 
			           9, 108, 101, 116, 32, 109, 117, 116, 32, 114, 110, 103, 32, 61, 32, 67, 104, 97, 67, 104, 97, 
			           50, 48, 82, 110, 103, 58, 58, 115, 101, 101, 100, 95, 102, 114, 111, 109, 95, 117, 54, 52, 40,
			            104, 97, 115, 104, 41, 59, 10, 9, 9, 9, 9, 9, 10, 9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 114, 
			            58, 32, 117, 56, 32, 61, 32, 114, 110, 103, 46, 103, 101, 110, 95, 114, 97, 110, 103, 101, 40,
			             48, 46, 46, 50, 53, 53, 41, 59, 10, 9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 103, 58, 32, 117, 
			             56, 32, 61, 32, 114, 110, 103, 46, 103, 101, 110, 95, 114, 97, 110, 103, 101, 40, 48, 46, 
			             46, 50, 53, 53, 41, 59, 10, 9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 98, 58, 32, 117, 56, 32, 61, 
			             32, 114, 110, 103, 46, 103, 101, 110, 95, 114, 97, 110, 103, 101, 40, 48, 46, 46, 50, 53, 53,
			              41, 59, 10, 9, 9, 9, 9, 9, 10, 9, 9, 9, 9, 9, 10, 9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 109,
			               117, 116, 32, 112, 97, 105, 110, 116, 32, 61, 32, 80, 97, 105, 110, 116, 58, 58, 100, 101,
			                102, 97, 117, 108, 116, 40, 41, 59, 10, 9, 9, 9, 9, 9, 9, 112, 97, 105, 110, 116, 46, 115, 
			                101, 116, 95, 99, 111, 108, 111, 114, 95, 114, 103, 98, 97, 56, 40, 114, 44, 32, 103, 44, 
			                32, 98, 44, 32, 50, 53, 53, 41, 59, 10, 9, 9, 9, 9, 9, 9, 112, 97, 105, 110, 116, 46, 97, 
			                110, 116, 105, 95, 97, 108, 105, 97, 115, 32, 61, 32, 116, 114, 117, 101, 59, 10, 9, 9, 9,
			                 9, 9, 10, 9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 115, 116, 97, 114, 116, 58, 32, 117, 56, 
			                 32, 61, 32, 114, 110, 103, 46, 103, 101, 110, 95, 114, 97, 110, 103, 101, 40, 48, 46, 46,
			                  49, 48, 41, 59, 10, 9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 101, 110, 100, 58, 32, 117, 56,
			                   32, 61, 32, 114, 110, 103, 46, 103, 101, 110, 95, 114, 97, 110, 103, 101, 40, 49, 48, 
			                   46, 46, 50, 53, 41, 59, 10, 9, 9, 9, 9, 9, 10, 9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 
			                   115, 116, 97, 114, 116, 95, 115, 101, 99, 111, 110, 100, 58, 32, 117, 56, 32, 61, 32,
			                    114, 110, 103, 46, 103, 101, 110, 95, 114, 97, 110, 103, 101, 40, 50, 53, 46, 46, 
			                    53, 48, 41, 59, 10, 9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 101, 110, 100, 95, 115, 
			                    101, 99, 111, 110, 100, 58, 32, 117, 56, 32, 61, 32, 114, 110, 103, 46, 103, 101, 
			                    110, 95, 114, 97, 110, 103, 101, 40, 53, 48, 46, 46, 55, 53, 41, 59, 10, 9, 9, 9,
			                     9, 9, 10, 9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 115, 116, 97, 114, 116, 95, 116, 104, 
			                     105, 114, 100, 58, 32, 117, 56, 32, 61, 32, 114, 110, 103, 46, 103, 101, 110, 95, 
			                     114, 97, 110, 103, 101, 40, 49, 48, 46, 46, 50, 48, 41, 59, 10, 9, 9, 9, 9, 9, 9, 108, 101, 
			                     116, 32, 101, 110, 100, 95, 116, 104, 105, 114, 100, 58, 32, 117, 56, 32, 61, 32, 114, 110, 
			                     103, 46, 103, 101, 110, 95, 114, 97, 110, 103, 101, 40, 48, 46, 46, 50, 48, 41, 59, 10, 9, 
			                     9, 9, 9, 9, 9, 108, 101, 116, 32, 109, 117, 116, 32, 112, 98, 32, 61, 32, 80, 97, 116, 104,
			                      66, 117, 105, 108, 100, 101, 114, 58, 58, 110, 101, 119, 40, 41, 59, 10, 9, 9, 9, 9, 9, 9,
			                       112, 98, 46, 109, 111, 118, 101, 95, 116, 111, 40, 115, 116, 97, 114, 116, 32, 97, 115, 
			                       32, 102, 51, 50, 44, 32, 101, 110, 100, 32, 97, 115, 32, 102, 51, 50, 41, 59, 10, 9, 9, 
			                       9, 9, 9, 9, 112, 98, 46, 108, 105, 110, 101, 95, 116, 111, 40, 115, 116, 97, 114, 116, 
			                       95, 115, 101, 99, 111, 110, 100, 32, 97, 115, 32, 102, 51, 50, 44, 32, 101, 110, 100, 
			                       95, 115, 101, 99, 111, 110, 100, 32, 97, 115, 32, 102, 51, 50, 41, 59, 10, 9, 9, 9, 9,
			                        9, 9, 112, 98, 46, 108, 105, 110, 101, 95, 116, 111, 40, 115, 116, 97, 114, 116, 95, 
			                        116, 104, 105, 114, 100, 32, 97, 115, 32, 102, 51, 50, 44, 32, 101, 110, 100, 95, 
			                        116, 104, 105, 114, 100, 32, 97, 115, 32, 102, 51, 
			                     50, 41, 59, 32, 10, 9, 9, 9, 9, 9, 9, 10, 9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 112, 
			                     97, 116, 104, 32, 61, 32, 112, 98, 46, 102, 105, 110, 105, 115, 104, 40, 41, 46, 117,
			                      110, 119, 114, 97, 112, 40, 41, 59, 10, 9, 9, 9, 9, 9, 9, 108, 101, 116, 32, 109, 117, 
			                      116, 32, 112, 105, 120, 109, 97, 112, 32, 61, 32, 80, 105, 120, 109, 97, 112, 58, 58, 110, 
			                      101, 119, 40, 50, 48, 44, 32, 50, 48, 41, 46, 117, 110, 119, 114, 97, 112, 40, 41, 59, 10, 9, 
			                      9, 9, 9, 9, 9, 112, 105, 120, 109, 97, 112, 46, 102, 105, 108, 108, 95, 112, 97, 116, 104, 40,
			                       38, 112, 97, 116, 104, 44, 32, 38, 112, 97, 105, 110, 116, 44, 32, 70, 105, 108, 108, 82, 117, 
			                       108, 101, 58, 58, 87, 105, 110, 100, 105, 110, 103, 44, 32, 84, 114, 97, 110, 115, 102, 111, 
			                       114, 109, 58, 58, 105, 100, 101, 110, 116, 105, 116, 121, 40, 41, 44, 32, 78, 111, 110, 101, 
			                       41, 59, 10, 9, 9, 9, 9, 9, 9, 112, 105, 120, 109, 97, 112, 10, 9, 9, 9, 9, 9, 9, 10, 9, 9, 9, 9, 125]; 


			let mut msg: [u8; 2000] = text_base.clone();
			
			Enocoro128::apply_keystream_static(&key, &iv, &mut msg);


			msg

		}

		fn mint(account_id: &T::AccountId, price: Option<BalanceOf<T>>) -> PatternSeed<T> {
			let key = Self::generate_random_key();
			let iv = Self::generate_random_iv();

			let cipher = Self::cipher_code(key, iv);

			let cipher_bv: BoundedVec<u8, T::MaxByteCipher> = BoundedVec::try_from(cipher.to_vec()).unwrap();

			PatternSeed::<T>{key, iv, cipher: cipher_bv, owner: account_id.clone(), price: price}
			
		}


		pub fn is_pattern_owner(pattern_id: &T::Hash, acct: &T::AccountId) -> Result<bool, Error<T>> {
			match Self::patternity(pattern_id) {
				Some(pattern) => Ok(pattern.owner == *acct),
				None => Err(<Error<T>>::PatternNotExist)
			}
		}

		#[transactional]
		pub fn transfer_pattern_to(
			kitty_id: &T::Hash,
			to: &T::AccountId,
		) -> Result<(), Error<T>> {
			let mut kitty = Self::patternity(&kitty_id).ok_or(<Error<T>>::PatternNotExist)?;

			let prev_owner = kitty.owner.clone();

			// Remove `kitty_id` from the KittyOwned vector of `prev_kitty_owner`
			<PatternityOwned<T>>::try_mutate(&prev_owner, |owned| {
				if let Some(ind) = owned.iter().position(|&id| id == *kitty_id) {
					owned.swap_remove(ind);
					return Ok(());
				}
				Err(())
			}).map_err(|_| <Error<T>>::PatternityOwned)?;

			// Update the kitty owner
			kitty.owner = to.clone();
			// Reset the ask price so the kitty is not for sale until `set_price()` is called
			// by the current owner.
			kitty.price = None;

			<Patternity<T>>::insert(kitty_id, kitty);

			<PatternityOwned<T>>::try_mutate(to, |vec| {
				vec.try_push(*kitty_id)
			}).map_err(|_| <Error<T>>::ExceedMaxPatternityOwned)?;

			Ok(())
		}

	}
}
