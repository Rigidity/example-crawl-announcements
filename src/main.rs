use std::{
    collections::{HashMap, HashSet},
    fs,
};

use chia::protocol::{Bytes, Bytes32};
use serde::{Deserialize, Serialize};
use serde_with::{hex::Hex, serde_as};
use sha2::{digest::FixedOutput, Digest, Sha256};

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Item {
    #[serde(rename = "Coin")]
    #[serde_as(as = "Hex")]
    coin_id: Bytes32,

    #[serde(rename = "Coin_puzzle_hash")]
    #[serde_as(as = "Option<Hex>")]
    puzzle_hash: Option<Bytes32>,

    #[serde(rename = "Type")]
    ty: String,

    #[serde(rename = "Tags")]
    tags: Option<Vec<String>>,

    #[serde(rename = "Spend")]
    spend: bool,

    #[serde(rename = "Conditions")]
    conditions: Vec<Condition>,

    #[serde(rename = "Children")]
    children: Vec<Item>,
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "opcode", rename_all = "SCREAMING_SNAKE_CASE")]
enum Condition {
    CreatePuzzleAnnouncement {
        #[serde_as(as = "Vec<Hex>")]
        vars: Vec<Bytes>,
    },
    CreateCoinAnnouncement {
        #[serde_as(as = "Vec<Hex>")]
        vars: Vec<Bytes>,
    },
    AssertPuzzleAnnouncement {
        #[serde_as(as = "Vec<Hex>")]
        vars: Vec<Bytes32>,
    },
    AssertCoinAnnouncement {
        #[serde_as(as = "Vec<Hex>")]
        vars: Vec<Bytes32>,
    },
    CreateCoin {
        #[serde_as(as = "Hex")]
        #[serde(rename = "send_puzzle")]
        puzzle_hash: Bytes32,

        #[serde(rename = "amt")]
        amount: u64,

        #[serde_as(as = "Hex")]
        #[serde(rename = "child_coin_name")]
        child_coin_id: Bytes32,

        #[serde(rename = "send_address")]
        address: String,
    },
    AssertMyCoinId {
        #[serde_as(as = "Vec<Hex>")]
        vars: Vec<Bytes32>,
    },
    AggSigMe {
        #[serde_as(as = "Vec<Hex>")]
        vars: Vec<Bytes>,
    },
    ReserveFee {
        #[serde_as(as = "Vec<Hex>")]
        vars: Vec<Bytes>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CreateCoinAnnouncement {
    coin_id: Bytes32,
    message: Bytes,
    announcement_id: Bytes32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CreatePuzzleAnnouncement {
    coin_id: Bytes32,
    puzzle_hash: Bytes32,
    message: Bytes,
    announcement_id: Bytes32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssertPuzzleAnnouncement {
    coin_id: Bytes32,
    announcement_id: Bytes32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssertCoinAnnouncement {
    coin_id: Bytes32,
    announcement_id: Bytes32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Announcements {
    create_coin: HashMap<Bytes32, CreateCoinAnnouncement>,
    create_puzzle: HashMap<Bytes32, CreatePuzzleAnnouncement>,
    assert_puzzle: Vec<AssertPuzzleAnnouncement>,
    assert_coin: Vec<AssertCoinAnnouncement>,
}

fn main() -> anyhow::Result<()> {
    let file = fs::read_to_string("block.json")?;
    let mut items: Vec<Item> = serde_json::from_str(&file)?;

    let mut create_coin_announcements = HashMap::<Bytes32, CreateCoinAnnouncement>::new();
    let mut create_puzzle_announcements = HashMap::<Bytes32, CreatePuzzleAnnouncement>::new();
    let mut assert_puzzle_announcements = vec![];
    let mut assert_coin_announcements = vec![];

    for item in items.clone() {
        items.extend(item.children);
    }

    for item in items.clone() {
        for condition in item.conditions {
            match condition {
                Condition::CreateCoinAnnouncement { vars } => {
                    let message = vars[0].clone();

                    let mut hasher = Sha256::new();
                    hasher.update(item.coin_id);
                    hasher.update(&message);

                    let announcement_id = Bytes32::new(hasher.finalize_fixed().into());

                    create_coin_announcements.insert(
                        announcement_id,
                        CreateCoinAnnouncement {
                            coin_id: item.coin_id,
                            message,
                            announcement_id,
                        },
                    );
                }
                Condition::CreatePuzzleAnnouncement { vars } => {
                    let message = vars[0].clone();

                    let mut hasher = Sha256::new();
                    hasher.update(item.puzzle_hash.unwrap());
                    hasher.update(&message);

                    let announcement_id = Bytes32::new(hasher.finalize_fixed().into());

                    create_puzzle_announcements.insert(
                        announcement_id,
                        CreatePuzzleAnnouncement {
                            coin_id: item.coin_id,
                            puzzle_hash: item.puzzle_hash.unwrap(),
                            message,
                            announcement_id,
                        },
                    );
                }
                Condition::AssertCoinAnnouncement { vars } => {
                    assert_coin_announcements.push(AssertCoinAnnouncement {
                        coin_id: item.coin_id,
                        announcement_id: vars[0],
                    });
                }
                Condition::AssertPuzzleAnnouncement { vars } => {
                    assert_puzzle_announcements.push(AssertPuzzleAnnouncement {
                        coin_id: item.coin_id,
                        announcement_id: vars[0],
                    });
                }
                _ => {}
            }
        }
    }

    let announcements = Announcements {
        create_coin: create_coin_announcements,
        create_puzzle: create_puzzle_announcements,
        assert_puzzle: assert_puzzle_announcements,
        assert_coin: assert_coin_announcements,
    };

    for item in items {
        if item
            .tags
            .unwrap_or_default()
            .contains(&"settlement_payments".to_string())
        {
            let coins = coins_asserted_by(item.coin_id, &announcements);
            println!("Coin {} is asserted by {:?}", item.coin_id, coins);
        }
    }

    Ok(())
}

fn coins_asserted_by(coin_id: Bytes32, announcements: &Announcements) -> HashSet<Bytes32> {
    let mut coins = HashSet::new();
    let mut stack = vec![coin_id];
    while let Some(coin_id) = stack.pop() {
        for asserted in coins_directly_asserted_by(coin_id, announcements) {
            if coins.insert(asserted) {
                stack.push(asserted);
            }
        }
    }
    coins
}

fn coins_directly_asserted_by(coin_id: Bytes32, announcements: &Announcements) -> HashSet<Bytes32> {
    let mut coins = HashSet::new();
    for created in announcements.create_coin.values() {
        if created.coin_id != coin_id {
            continue;
        }
        for asserted in announcements.assert_coin.iter() {
            if created.announcement_id == asserted.announcement_id {
                coins.insert(asserted.coin_id);
            }
        }
    }
    for created in announcements.create_puzzle.values() {
        if created.coin_id != coin_id {
            continue;
        }
        for asserted in announcements.assert_puzzle.iter() {
            if created.announcement_id == asserted.announcement_id {
                coins.insert(asserted.coin_id);
            }
        }
    }
    coins
}
