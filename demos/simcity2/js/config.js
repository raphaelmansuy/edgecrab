// config.js — tile types, building catalog, and tunable simulation constants.
// Pure data + small helpers. No DOM access here.

export const GRID_W = 40;
export const GRID_H = 40;
export const TILE = 28; // base pixel size per tile (zoomable)

// Tool / building catalog. Cost is one-time build cost.
// upkeep is $/month. power is power produced (negative = consumed).
// pop/jobs are capacities that the sim fills over time.
export const BUILDINGS = {
  residential: {
    name: "Residential Zone",
    icon: "🏠",
    cost: 100,
    upkeep: 0,
    power: -2,
    popCap: 40,
    needsRoad: true,
    needsPower: true,
    color: "#3fa66a",
    desc: "Houses citizens. Needs power and road access to grow.",
  },
  commercial: {
    name: "Commercial Zone",
    icon: "🏢",
    cost: 150,
    upkeep: 0,
    power: -3,
    jobCap: 30,
    needsRoad: true,
    needsPower: true,
    color: "#3a7bd5",
    desc: "Provides jobs & tax income. Needs power and road access.",
  },
  industrial: {
    name: "Industrial Zone",
    icon: "🏭",
    cost: 180,
    upkeep: 0,
    power: -5,
    jobCap: 45,
    needsRoad: true,
    needsPower: true,
    color: "#b07a2a",
    desc: "Heavy jobs & high tax, but lowers nearby happiness.",
  },
  road: {
    name: "Road",
    icon: "🛣️",
    cost: 25,
    upkeep: 1,
    power: 0,
    needsRoad: false,
    needsPower: false,
    color: "#5b6172",
    desc: "Connects zones to the city. Required for growth.",
  },
  park: {
    name: "Park",
    icon: "🌳",
    cost: 60,
    upkeep: 2,
    power: 0,
    needsRoad: false,
    needsPower: false,
    color: "#2f8f4e",
    happiness: 4,
    desc: "Boosts happiness of nearby tiles.",
  },
  power: {
    name: "Power Plant",
    icon: "⚡",
    cost: 1200,
    upkeep: 30,
    power: 120,
    needsRoad: false,
    needsPower: false,
    color: "#d9a441",
    desc: "Generates 120 power. Keep supply above demand.",
  },
  police: {
    name: "Police Station",
    icon: "🚓",
    cost: 500,
    upkeep: 20,
    power: -4,
    needsRoad: true,
    needsPower: true,
    coverage: 8,
    color: "#2c3e66",
    desc: "Reduces crime in a radius. Lowers crime unhappiness.",
  },
  hospital: {
    name: "Hospital",
    icon: "🏥",
    cost: 700,
    upkeep: 25,
    power: -6,
    needsRoad: true,
    needsPower: true,
    coverage: 10,
    color: "#b04a5a",
    desc: "Improves health & approval over a wide radius.",
  },
  school: {
    name: "School",
    icon: "🏫",
    cost: 400,
    upkeep: 15,
    power: -3,
    needsRoad: true,
    needsPower: true,
    coverage: 7,
    color: "#7a5aa8",
    desc: "Raises education & approval for nearby residents.",
  },
  bulldozer: {
    name: "Bulldozer",
    icon: "🧹",
    cost: 0,
    upkeep: 0,
    power: 0,
    needsRoad: false,
    needsPower: false,
    color: "#888",
    desc: "Raze a tile. Refunds 25% of its build cost.",
  },
};

// Default economy values.
export const START_MONEY = 50000;
export const DEFAULT_TAX = 7; // percent of assessed value collected monthly
export const MONTHS = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];

// Happiness factors (each contributes points, clamped 0..100).
export const HAPPY = {
  base: 70,
  perPark: 1.2,
  perUnpowered: -8,
  perNoRoad: -6,
  perIndustrialNear: -0.6,
  perPolice: 2,
  perHospital: 2.5,
  perSchool: 2,
  crimePenalty: -10,
  unemploymentPenalty: -0.4, // per percent of jobless residents
};

// Lose condition: treasury below this for too long.
export const BANKRUPT_LIMIT = -20000;
