//! The skills, super abilities and tools of mcMMO, as fixed tables (PrimarySkillType, SuperAbilityType, ToolType).

/// mcMMO's PrimarySkillType, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Skill {
    Acrobatics,
    Alchemy,
    Archery,
    Axes,
    Crossbows,
    Excavation,
    Fishing,
    Herbalism,
    Maces,
    Mining,
    Repair,
    Salvage,
    Smelting,
    Spears,
    Swords,
    Taming,
    Tridents,
    Unarmed,
    Woodcutting,
}

pub const COUNT: usize = 19;

pub const ALL: [Skill; COUNT] = [
    Skill::Acrobatics,
    Skill::Alchemy,
    Skill::Archery,
    Skill::Axes,
    Skill::Crossbows,
    Skill::Excavation,
    Skill::Fishing,
    Skill::Herbalism,
    Skill::Maces,
    Skill::Mining,
    Skill::Repair,
    Skill::Salvage,
    Skill::Smelting,
    Skill::Spears,
    Skill::Swords,
    Skill::Taming,
    Skill::Tridents,
    Skill::Unarmed,
    Skill::Woodcutting,
];

impl Skill {
    pub fn idx(self) -> usize {
        self as usize
    }
    /// As configs write it: `Mining`.
    pub fn name(self) -> &'static str {
        match self {
            Skill::Acrobatics => "Acrobatics",
            Skill::Alchemy => "Alchemy",
            Skill::Archery => "Archery",
            Skill::Axes => "Axes",
            Skill::Crossbows => "Crossbows",
            Skill::Excavation => "Excavation",
            Skill::Fishing => "Fishing",
            Skill::Herbalism => "Herbalism",
            Skill::Maces => "Maces",
            Skill::Mining => "Mining",
            Skill::Repair => "Repair",
            Skill::Salvage => "Salvage",
            Skill::Smelting => "Smelting",
            Skill::Spears => "Spears",
            Skill::Swords => "Swords",
            Skill::Taming => "Taming",
            Skill::Tridents => "Tridents",
            Skill::Unarmed => "Unarmed",
            Skill::Woodcutting => "Woodcutting",
        }
    }
    /// `mining`, for permissions and placeholders.
    pub fn lower(self) -> &'static str {
        match self {
            Skill::Acrobatics => "acrobatics",
            Skill::Alchemy => "alchemy",
            Skill::Archery => "archery",
            Skill::Axes => "axes",
            Skill::Crossbows => "crossbows",
            Skill::Excavation => "excavation",
            Skill::Fishing => "fishing",
            Skill::Herbalism => "herbalism",
            Skill::Maces => "maces",
            Skill::Mining => "mining",
            Skill::Repair => "repair",
            Skill::Salvage => "salvage",
            Skill::Smelting => "smelting",
            Skill::Spears => "spears",
            Skill::Swords => "swords",
            Skill::Taming => "taming",
            Skill::Tridents => "tridents",
            Skill::Unarmed => "unarmed",
            Skill::Woodcutting => "woodcutting",
        }
    }
    pub fn parse(name: &str) -> Option<Skill> {
        let lower = name.to_ascii_lowercase();
        ALL.iter().copied().find(|s| s.lower() == lower).or_else(|| {
            // Localized names (Overhaul.Name.X) and the old ones.
            ALL.iter().copied().find(|s| crate::locale::get(&format!("Overhaul.Name.{}", s.name()), &[]).eq_ignore_ascii_case(name))
        })
    }
    pub fn is_child(self) -> bool {
        matches!(self, Skill::Salvage | Skill::Smelting)
    }
    pub fn parents(self) -> &'static [Skill] {
        match self {
            Skill::Salvage => &[Skill::Repair, Skill::Fishing],
            Skill::Smelting => &[Skill::Mining, Skill::Repair],
            _ => &[],
        }
    }
    /// The skills that are not children (what levels and leaderboards count).
    pub fn non_child() -> impl Iterator<Item = Skill> {
        ALL.iter().copied().filter(|s| !s.is_child())
    }
    pub fn is_combat(self) -> bool {
        matches!(self, Skill::Archery | Skill::Axes | Skill::Crossbows | Skill::Maces | Skill::Spears | Skill::Swords | Skill::Taming | Skill::Tridents | Skill::Unarmed)
    }
    pub fn super_ability(self) -> Option<Ability> {
        Some(match self {
            Skill::Unarmed => Ability::Berserk,
            Skill::Mining => Ability::SuperBreaker,
            Skill::Excavation => Ability::GigaDrillBreaker,
            Skill::Herbalism => Ability::GreenTerra,
            Skill::Swords => Ability::SerratedStrikes,
            Skill::Axes => Ability::SkullSplitter,
            Skill::Woodcutting => Ability::TreeFeller,
            _ => return None,
        })
    }
    pub fn tool(self) -> Option<Tool> {
        Some(match self {
            Skill::Unarmed => Tool::Fists,
            Skill::Mining => Tool::Pickaxe,
            Skill::Excavation => Tool::Shovel,
            Skill::Herbalism => Tool::Hoe,
            Skill::Swords => Tool::Sword,
            Skill::Axes | Skill::Woodcutting => Tool::Axe,
            Skill::Crossbows => Tool::Crossbow,
            Skill::Tridents => Tool::Trident,
            Skill::Maces => Tool::Mace,
            Skill::Spears => Tool::Spear,
            _ => return None,
        })
    }
}

/// SuperAbilityType (the ones that do something; the newer skills' are placeholders in mcMMO too).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Ability {
    Berserk,
    SuperBreaker,
    GigaDrillBreaker,
    GreenTerra,
    SerratedStrikes,
    SkullSplitter,
    TreeFeller,
    BlastMining,
}

pub const ABILITIES: [Ability; 8] = [Ability::Berserk, Ability::SuperBreaker, Ability::GigaDrillBreaker, Ability::GreenTerra, Ability::SerratedStrikes, Ability::SkullSplitter, Ability::TreeFeller, Ability::BlastMining];

impl Ability {
    pub fn idx(self) -> usize {
        self as usize
    }
    /// Its key in config.yml (Abilities.Cooldowns.X).
    pub fn config_key(self) -> &'static str {
        match self {
            Ability::Berserk => "Berserk",
            Ability::SuperBreaker => "Super_Breaker",
            Ability::GigaDrillBreaker => "Giga_Drill_Breaker",
            Ability::GreenTerra => "Green_Terra",
            Ability::SerratedStrikes => "Serrated_Strikes",
            Ability::SkullSplitter => "Skull_Splitter",
            Ability::TreeFeller => "Tree_Feller",
            Ability::BlastMining => "Blast_Mining",
        }
    }
    pub fn skill(self) -> Skill {
        match self {
            Ability::Berserk => Skill::Unarmed,
            Ability::SuperBreaker | Ability::BlastMining => Skill::Mining,
            Ability::GigaDrillBreaker => Skill::Excavation,
            Ability::GreenTerra => Skill::Herbalism,
            Ability::SerratedStrikes => Skill::Swords,
            Ability::SkullSplitter => Skill::Axes,
            Ability::TreeFeller => Skill::Woodcutting,
        }
    }
    /// The locale key stem: `Mining.Skills.SuperBreaker` (+ .On/.Off/.Other.On/.Refresh).
    pub fn locale(self) -> &'static str {
        match self {
            Ability::Berserk => "Unarmed.Skills.Berserk",
            Ability::SuperBreaker => "Mining.Skills.SuperBreaker",
            Ability::GigaDrillBreaker => "Excavation.Skills.GigaDrillBreaker",
            Ability::GreenTerra => "Herbalism.Skills.GTe",
            Ability::SerratedStrikes => "Swords.Skills.SS",
            Ability::SkullSplitter => "Axes.Skills.SS",
            Ability::TreeFeller => "Woodcutting.Skills.TreeFeller",
            Ability::BlastMining => "Mining.Blast",
        }
    }
    /// Its name as players read it (SubSkill name in the locale).
    pub fn display(self) -> String {
        let key = match self {
            Ability::Berserk => "Unarmed.SubSkill.Berserk.Name",
            Ability::SuperBreaker => "Mining.SubSkill.SuperBreaker.Name",
            Ability::GigaDrillBreaker => "Excavation.SubSkill.GigaDrillBreaker.Name",
            Ability::GreenTerra => "Herbalism.SubSkill.GreenTerra.Name",
            Ability::SerratedStrikes => "Swords.SubSkill.SerratedStrikes.Name",
            Ability::SkullSplitter => "Axes.SubSkill.SkullSplitter.Name",
            Ability::TreeFeller => "Woodcutting.SubSkill.TreeFeller.Name",
            Ability::BlastMining => "Mining.SubSkill.BlastMining.Name",
        };
        crate::locale::get(key, &[])
    }
    /// The skillranks.yml entry that unlocks it.
    pub fn rank_key(self) -> &'static str {
        match self {
            Ability::Berserk => "Unarmed.Berserk",
            Ability::SuperBreaker => "Mining.SuperBreaker",
            Ability::GigaDrillBreaker => "Excavation.GigaDrillBreaker",
            Ability::GreenTerra => "Herbalism.GreenTerra",
            Ability::SerratedStrikes => "Swords.SerratedStrikes",
            Ability::SkullSplitter => "Axes.SkullSplitter",
            Ability::TreeFeller => "Woodcutting.TreeFeller",
            Ability::BlastMining => "Mining.BlastMining",
        }
    }
    /// The permission node mcMMO checks.
    pub fn perm(self) -> &'static str {
        match self {
            Ability::Berserk => "mcmmo.ability.unarmed.berserk",
            Ability::SuperBreaker => "mcmmo.ability.mining.superbreaker",
            Ability::GigaDrillBreaker => "mcmmo.ability.excavation.gigadrillbreaker",
            Ability::GreenTerra => "mcmmo.ability.herbalism.greenterra",
            Ability::SerratedStrikes => "mcmmo.ability.swords.serratedstrikes",
            Ability::SkullSplitter => "mcmmo.ability.axes.skullsplitter",
            Ability::TreeFeller => "mcmmo.ability.woodcutting.treefeller",
            Ability::BlastMining => "mcmmo.ability.mining.blastmining.detonate",
        }
    }
}

/// ToolType.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Pickaxe,
    Axe,
    Shovel,
    Hoe,
    Sword,
    Fists,
    Bow,
    Crossbow,
    Trident,
    Mace,
    Spear,
}

impl Tool {
    /// What a held item (its id, `minecraft:diamond_pickaxe`; empty for a bare hand) is.
    pub fn of(item: &str) -> Option<Tool> {
        let id = item.trim_start_matches("minecraft:");
        if id.is_empty() || id == "air" {
            return Some(Tool::Fists);
        }
        Some(if id.ends_with("_pickaxe") {
            Tool::Pickaxe
        } else if id.ends_with("_axe") {
            Tool::Axe
        } else if id.ends_with("_shovel") {
            Tool::Shovel
        } else if id.ends_with("_hoe") {
            Tool::Hoe
        } else if id.ends_with("_sword") {
            Tool::Sword
        } else if id.ends_with("_spear") {
            Tool::Spear
        } else {
            match id {
                "bow" => Tool::Bow,
                "crossbow" => Tool::Crossbow,
                "trident" => Tool::Trident,
                "mace" => Tool::Mace,
                _ => return None,
            }
        })
    }
    /// The locale stem for "You ready / lower your X".
    pub fn locale(self) -> &'static str {
        match self {
            Tool::Pickaxe => "Mining.Ability",
            Tool::Axe => "Axes.Ability",
            Tool::Shovel => "Excavation.Ability",
            Tool::Hoe => "Herbalism.Ability",
            Tool::Sword => "Swords.Ability",
            Tool::Fists => "Unarmed.Ability",
            Tool::Bow => "Archery.Ability",
            Tool::Crossbow => "Crossbows.Ability",
            Tool::Trident => "Tridents.Ability",
            Tool::Mace => "Maces.Ability",
            Tool::Spear => "Spears.Ability",
        }
    }
}

/// The tier of a tool or armour piece by its material, for Repair's XP multipliers.
pub fn tier(item: &str) -> &'static str {
    let id = item.trim_start_matches("minecraft:");
    if id.starts_with("wooden_") || id == "bow" || id == "crossbow" || id == "fishing_rod" || id == "shield" {
        if id == "bow" || id == "crossbow" || id == "fishing_rod" {
            return "String";
        }
        return "Wood";
    }
    if id.starts_with("stone_") {
        "Stone"
    } else if id.starts_with("copper_") {
        "Copper"
    } else if id.starts_with("iron_") || id.starts_with("chainmail_") {
        "Iron"
    } else if id.starts_with("golden_") {
        "Gold"
    } else if id.starts_with("diamond_") {
        "Diamond"
    } else if id.starts_with("netherite_") {
        "Netherite"
    } else if id.starts_with("leather_") {
        "Leather"
    } else {
        "Other"
    }
}
