pub struct BrickColorInfo {
    pub number: u16,
    pub rgb: [u8; 3],
    pub name: &'static str,
}

pub const ALL_COLORS: [BrickColorInfo; 209] = [
    BrickColorInfo {
        number: 1,
        rgb: [242, 243, 243],
        name: "White",
    },
    BrickColorInfo {
        number: 2,
        rgb: [161, 165, 162],
        name: "Grey",
    },
    BrickColorInfo {
        number: 3,
        rgb: [249, 233, 153],
        name: "Light yellow",
    },
    BrickColorInfo {
        number: 5,
        rgb: [215, 197, 154],
        name: "Brick yellow",
    },
    BrickColorInfo {
        number: 6,
        rgb: [194, 218, 184],
        name: "Light green (Mint)",
    },
    BrickColorInfo {
        number: 9,
        rgb: [232, 186, 200],
        name: "Light reddish violet",
    },
    BrickColorInfo {
        number: 11,
        rgb: [128, 187, 219],
        name: "Pastel Blue",
    },
    BrickColorInfo {
        number: 12,
        rgb: [203, 132, 66],
        name: "Light orange brown",
    },
    BrickColorInfo {
        number: 18,
        rgb: [204, 142, 105],
        name: "Nougat",
    },
    BrickColorInfo {
        number: 21,
        rgb: [196, 40, 28],
        name: "Bright red",
    },
    BrickColorInfo {
        number: 22,
        rgb: [196, 112, 160],
        name: "Med. reddish violet",
    },
    BrickColorInfo {
        number: 23,
        rgb: [13, 105, 172],
        name: "Bright blue",
    },
    BrickColorInfo {
        number: 24,
        rgb: [245, 205, 48],
        name: "Bright yellow",
    },
    BrickColorInfo {
        number: 25,
        rgb: [98, 71, 50],
        name: "Earth orange",
    },
    BrickColorInfo {
        number: 26,
        rgb: [27, 42, 53],
        name: "Black",
    },
    BrickColorInfo {
        number: 27,
        rgb: [109, 110, 108],
        name: "Dark grey",
    },
    BrickColorInfo {
        number: 28,
        rgb: [40, 127, 71],
        name: "Dark green",
    },
    BrickColorInfo {
        number: 29,
        rgb: [161, 196, 140],
        name: "Medium green",
    },
    BrickColorInfo {
        number: 36,
        rgb: [243, 207, 155],
        name: "Lig. Yellowich orange",
    },
    BrickColorInfo {
        number: 37,
        rgb: [75, 151, 75],
        name: "Bright green",
    },
    BrickColorInfo {
        number: 38,
        rgb: [160, 95, 53],
        name: "Dark orange",
    },
    BrickColorInfo {
        number: 39,
        rgb: [193, 202, 222],
        name: "Light bluish violet",
    },
    BrickColorInfo {
        number: 40,
        rgb: [236, 236, 236],
        name: "Transparent",
    },
    BrickColorInfo {
        number: 41,
        rgb: [205, 84, 75],
        name: "Tr. Red",
    },
    BrickColorInfo {
        number: 42,
        rgb: [193, 223, 240],
        name: "Tr. Lg blue",
    },
    BrickColorInfo {
        number: 43,
        rgb: [123, 182, 232],
        name: "Tr. Blue",
    },
    BrickColorInfo {
        number: 44,
        rgb: [247, 241, 141],
        name: "Tr. Yellow",
    },
    BrickColorInfo {
        number: 45,
        rgb: [180, 210, 228],
        name: "Light blue",
    },
    BrickColorInfo {
        number: 47,
        rgb: [217, 133, 108],
        name: "Tr. Flu. Reddish orange",
    },
    BrickColorInfo {
        number: 48,
        rgb: [132, 182, 141],
        name: "Tr. Green",
    },
    BrickColorInfo {
        number: 49,
        rgb: [248, 241, 132],
        name: "Tr. Flu. Green",
    },
    BrickColorInfo {
        number: 50,
        rgb: [236, 232, 222],
        name: "Phosph. White",
    },
    BrickColorInfo {
        number: 100,
        rgb: [238, 196, 182],
        name: "Light red",
    },
    BrickColorInfo {
        number: 101,
        rgb: [218, 134, 122],
        name: "Medium red",
    },
    BrickColorInfo {
        number: 102,
        rgb: [110, 153, 202],
        name: "Medium blue",
    },
    BrickColorInfo {
        number: 103,
        rgb: [199, 193, 183],
        name: "Light grey",
    },
    BrickColorInfo {
        number: 104,
        rgb: [107, 50, 124],
        name: "Bright violet",
    },
    BrickColorInfo {
        number: 105,
        rgb: [226, 155, 64],
        name: "Br. yellowish orange",
    },
    BrickColorInfo {
        number: 106,
        rgb: [218, 133, 65],
        name: "Bright orange",
    },
    BrickColorInfo {
        number: 107,
        rgb: [0, 143, 156],
        name: "Bright bluish green",
    },
    BrickColorInfo {
        number: 108,
        rgb: [104, 92, 67],
        name: "Earth yellow",
    },
    BrickColorInfo {
        number: 110,
        rgb: [67, 84, 147],
        name: "Bright bluish violet",
    },
    BrickColorInfo {
        number: 111,
        rgb: [191, 183, 177],
        name: "Tr. Brown",
    },
    BrickColorInfo {
        number: 112,
        rgb: [104, 116, 172],
        name: "Medium bluish violet",
    },
    BrickColorInfo {
        number: 113,
        rgb: [228, 173, 200],
        name: "Tr. Medi. reddish violet",
    },
    BrickColorInfo {
        number: 115,
        rgb: [199, 210, 60],
        name: "Med. yellowish green",
    },
    BrickColorInfo {
        number: 116,
        rgb: [85, 165, 175],
        name: "Med. bluish green",
    },
    BrickColorInfo {
        number: 118,
        rgb: [183, 215, 213],
        name: "Light bluish green",
    },
    BrickColorInfo {
        number: 119,
        rgb: [164, 189, 71],
        name: "Br. yellowish green",
    },
    BrickColorInfo {
        number: 120,
        rgb: [217, 228, 167],
        name: "Lig. yellowish green",
    },
    BrickColorInfo {
        number: 121,
        rgb: [231, 172, 88],
        name: "Med. yellowish orange",
    },
    BrickColorInfo {
        number: 123,
        rgb: [211, 111, 76],
        name: "Br. reddish orange",
    },
    BrickColorInfo {
        number: 124,
        rgb: [146, 57, 120],
        name: "Bright reddish violet",
    },
    BrickColorInfo {
        number: 125,
        rgb: [234, 184, 146],
        name: "Light orange",
    },
    BrickColorInfo {
        number: 126,
        rgb: [165, 165, 203],
        name: "Tr. Bright bluish violet",
    },
    BrickColorInfo {
        number: 127,
        rgb: [220, 188, 129],
        name: "Gold",
    },
    BrickColorInfo {
        number: 128,
        rgb: [174, 122, 89],
        name: "Dark nougat",
    },
    BrickColorInfo {
        number: 131,
        rgb: [156, 163, 168],
        name: "Silver",
    },
    BrickColorInfo {
        number: 133,
        rgb: [213, 115, 61],
        name: "Neon orange",
    },
    BrickColorInfo {
        number: 134,
        rgb: [216, 221, 86],
        name: "Neon green",
    },
    BrickColorInfo {
        number: 135,
        rgb: [116, 134, 157],
        name: "Sand blue",
    },
    BrickColorInfo {
        number: 136,
        rgb: [135, 124, 144],
        name: "Sand violet",
    },
    BrickColorInfo {
        number: 137,
        rgb: [224, 152, 100],
        name: "Medium orange",
    },
    BrickColorInfo {
        number: 138,
        rgb: [149, 138, 115],
        name: "Sand yellow",
    },
    BrickColorInfo {
        number: 140,
        rgb: [32, 58, 86],
        name: "Earth blue",
    },
    BrickColorInfo {
        number: 141,
        rgb: [39, 70, 45],
        name: "Earth green",
    },
    BrickColorInfo {
        number: 143,
        rgb: [207, 226, 247],
        name: "Tr. Flu. Blue",
    },
    BrickColorInfo {
        number: 145,
        rgb: [121, 136, 161],
        name: "Sand blue metallic",
    },
    BrickColorInfo {
        number: 146,
        rgb: [149, 142, 163],
        name: "Sand violet metallic",
    },
    BrickColorInfo {
        number: 147,
        rgb: [147, 135, 103],
        name: "Sand yellow metallic",
    },
    BrickColorInfo {
        number: 148,
        rgb: [87, 88, 87],
        name: "Dark grey metallic",
    },
    BrickColorInfo {
        number: 149,
        rgb: [22, 29, 50],
        name: "Black metallic",
    },
    BrickColorInfo {
        number: 150,
        rgb: [171, 173, 172],
        name: "Light grey metallic",
    },
    BrickColorInfo {
        number: 151,
        rgb: [120, 144, 130],
        name: "Sand green",
    },
    BrickColorInfo {
        number: 153,
        rgb: [149, 121, 119],
        name: "Sand red",
    },
    BrickColorInfo {
        number: 154,
        rgb: [123, 46, 47],
        name: "Dark red",
    },
    BrickColorInfo {
        number: 157,
        rgb: [255, 246, 123],
        name: "Tr. Flu. Yellow",
    },
    BrickColorInfo {
        number: 158,
        rgb: [225, 164, 194],
        name: "Tr. Flu. Red",
    },
    BrickColorInfo {
        number: 168,
        rgb: [117, 108, 98],
        name: "Gun metallic",
    },
    BrickColorInfo {
        number: 176,
        rgb: [151, 105, 91],
        name: "Red flip/flop",
    },
    BrickColorInfo {
        number: 178,
        rgb: [180, 132, 85],
        name: "Yellow flip/flop",
    },
    BrickColorInfo {
        number: 179,
        rgb: [137, 135, 136],
        name: "Silver flip/flop",
    },
    BrickColorInfo {
        number: 180,
        rgb: [215, 169, 75],
        name: "Curry",
    },
    BrickColorInfo {
        number: 190,
        rgb: [249, 214, 46],
        name: "Fire Yellow",
    },
    BrickColorInfo {
        number: 191,
        rgb: [232, 171, 45],
        name: "Flame yellowish orange",
    },
    BrickColorInfo {
        number: 192,
        rgb: [105, 64, 40],
        name: "Reddish brown",
    },
    BrickColorInfo {
        number: 193,
        rgb: [207, 96, 36],
        name: "Flame reddish orange",
    },
    BrickColorInfo {
        number: 194,
        rgb: [163, 162, 165],
        name: "Medium stone grey",
    },
    BrickColorInfo {
        number: 195,
        rgb: [70, 103, 164],
        name: "Royal blue",
    },
    BrickColorInfo {
        number: 196,
        rgb: [35, 71, 139],
        name: "Dark Royal blue",
    },
    BrickColorInfo {
        number: 198,
        rgb: [142, 66, 133],
        name: "Bright reddish lilac",
    },
    BrickColorInfo {
        number: 199,
        rgb: [99, 95, 98],
        name: "Dark stone grey",
    },
    BrickColorInfo {
        number: 200,
        rgb: [130, 138, 93],
        name: "Lemon metalic",
    },
    BrickColorInfo {
        number: 208,
        rgb: [229, 228, 223],
        name: "Light stone grey",
    },
    BrickColorInfo {
        number: 209,
        rgb: [176, 142, 68],
        name: "Dark Curry",
    },
    BrickColorInfo {
        number: 210,
        rgb: [112, 149, 120],
        name: "Faded green",
    },
    BrickColorInfo {
        number: 211,
        rgb: [121, 181, 181],
        name: "Turquoise",
    },
    BrickColorInfo {
        number: 212,
        rgb: [159, 195, 233],
        name: "Light Royal blue",
    },
    BrickColorInfo {
        number: 213,
        rgb: [108, 129, 183],
        name: "Medium Royal blue",
    },
    BrickColorInfo {
        number: 216,
        rgb: [143, 76, 42],
        name: "Rust",
    },
    BrickColorInfo {
        number: 217,
        rgb: [124, 92, 70],
        name: "Brown",
    },
    BrickColorInfo {
        number: 218,
        rgb: [150, 112, 159],
        name: "Reddish lilac",
    },
    BrickColorInfo {
        number: 219,
        rgb: [107, 98, 155],
        name: "Lilac",
    },
    BrickColorInfo {
        number: 220,
        rgb: [167, 169, 206],
        name: "Light lilac",
    },
    BrickColorInfo {
        number: 221,
        rgb: [205, 98, 152],
        name: "Bright purple",
    },
    BrickColorInfo {
        number: 222,
        rgb: [228, 173, 200],
        name: "Light purple",
    },
    BrickColorInfo {
        number: 223,
        rgb: [220, 144, 149],
        name: "Light pink",
    },
    BrickColorInfo {
        number: 224,
        rgb: [240, 213, 160],
        name: "Light brick yellow",
    },
    BrickColorInfo {
        number: 225,
        rgb: [235, 184, 127],
        name: "Warm yellowish orange",
    },
    BrickColorInfo {
        number: 226,
        rgb: [253, 234, 141],
        name: "Cool yellow",
    },
    BrickColorInfo {
        number: 232,
        rgb: [125, 187, 221],
        name: "Dove blue",
    },
    BrickColorInfo {
        number: 268,
        rgb: [52, 43, 117],
        name: "Medium lilac",
    },
    BrickColorInfo {
        number: 301,
        rgb: [80, 109, 84],
        name: "Slime green",
    },
    BrickColorInfo {
        number: 302,
        rgb: [91, 93, 105],
        name: "Smoky grey",
    },
    BrickColorInfo {
        number: 303,
        rgb: [0, 16, 176],
        name: "Dark blue",
    },
    BrickColorInfo {
        number: 304,
        rgb: [44, 101, 29],
        name: "Parsley green",
    },
    BrickColorInfo {
        number: 305,
        rgb: [82, 124, 174],
        name: "Steel blue",
    },
    BrickColorInfo {
        number: 306,
        rgb: [51, 88, 130],
        name: "Storm blue",
    },
    BrickColorInfo {
        number: 307,
        rgb: [16, 42, 220],
        name: "Lapis",
    },
    BrickColorInfo {
        number: 308,
        rgb: [61, 21, 133],
        name: "Dark indigo",
    },
    BrickColorInfo {
        number: 309,
        rgb: [52, 142, 64],
        name: "Sea green",
    },
    BrickColorInfo {
        number: 310,
        rgb: [91, 154, 76],
        name: "Shamrock",
    },
    BrickColorInfo {
        number: 311,
        rgb: [159, 161, 172],
        name: "Fossil",
    },
    BrickColorInfo {
        number: 312,
        rgb: [89, 34, 89],
        name: "Mulberry",
    },
    BrickColorInfo {
        number: 313,
        rgb: [31, 128, 29],
        name: "Forest green",
    },
    BrickColorInfo {
        number: 314,
        rgb: [159, 173, 192],
        name: "Cadet blue",
    },
    BrickColorInfo {
        number: 315,
        rgb: [9, 137, 207],
        name: "Electric blue",
    },
    BrickColorInfo {
        number: 316,
        rgb: [123, 0, 123],
        name: "Eggplant",
    },
    BrickColorInfo {
        number: 317,
        rgb: [124, 156, 107],
        name: "Moss",
    },
    BrickColorInfo {
        number: 318,
        rgb: [138, 171, 133],
        name: "Artichoke",
    },
    BrickColorInfo {
        number: 319,
        rgb: [185, 196, 177],
        name: "Sage green",
    },
    BrickColorInfo {
        number: 320,
        rgb: [202, 203, 209],
        name: "Ghost grey",
    },
    BrickColorInfo {
        number: 321,
        rgb: [167, 94, 155],
        name: "Lilac",
    },
    BrickColorInfo {
        number: 322,
        rgb: [123, 47, 123],
        name: "Plum",
    },
    BrickColorInfo {
        number: 323,
        rgb: [148, 190, 129],
        name: "Olivine",
    },
    BrickColorInfo {
        number: 324,
        rgb: [168, 189, 153],
        name: "Laurel green",
    },
    BrickColorInfo {
        number: 325,
        rgb: [223, 223, 222],
        name: "Quill grey",
    },
    BrickColorInfo {
        number: 326,
        rgb: [218, 220, 225],
        name: "Iron",
    },
    BrickColorInfo {
        number: 327,
        rgb: [151, 0, 0],
        name: "Crimson",
    },
    BrickColorInfo {
        number: 328,
        rgb: [177, 229, 166],
        name: "Mint",
    },
    BrickColorInfo {
        number: 329,
        rgb: [152, 194, 219],
        name: "Baby blue",
    },
    BrickColorInfo {
        number: 330,
        rgb: [255, 152, 220],
        name: "Carnation pink",
    },
    BrickColorInfo {
        number: 331,
        rgb: [255, 89, 89],
        name: "Persimmon",
    },
    BrickColorInfo {
        number: 332,
        rgb: [117, 0, 0],
        name: "Maroon",
    },
    BrickColorInfo {
        number: 333,
        rgb: [239, 184, 56],
        name: "Gold",
    },
    BrickColorInfo {
        number: 334,
        rgb: [248, 217, 109],
        name: "Daisy orange",
    },
    BrickColorInfo {
        number: 335,
        rgb: [231, 231, 236],
        name: "Pearl",
    },
    BrickColorInfo {
        number: 336,
        rgb: [199, 212, 228],
        name: "Fog",
    },
    BrickColorInfo {
        number: 337,
        rgb: [255, 148, 148],
        name: "Salmon",
    },
    BrickColorInfo {
        number: 338,
        rgb: [190, 104, 98],
        name: "Terra Cotta",
    },
    BrickColorInfo {
        number: 339,
        rgb: [86, 36, 36],
        name: "Cocoa",
    },
    BrickColorInfo {
        number: 340,
        rgb: [241, 231, 199],
        name: "Wheat",
    },
    BrickColorInfo {
        number: 341,
        rgb: [254, 243, 187],
        name: "Buttermilk",
    },
    BrickColorInfo {
        number: 342,
        rgb: [224, 178, 208],
        name: "Mauve",
    },
    BrickColorInfo {
        number: 343,
        rgb: [212, 144, 189],
        name: "Sunrise",
    },
    BrickColorInfo {
        number: 344,
        rgb: [150, 85, 85],
        name: "Tawny",
    },
    BrickColorInfo {
        number: 345,
        rgb: [143, 76, 42],
        name: "Rust",
    },
    BrickColorInfo {
        number: 346,
        rgb: [211, 190, 150],
        name: "Cashmere",
    },
    BrickColorInfo {
        number: 347,
        rgb: [226, 220, 188],
        name: "Khaki",
    },
    BrickColorInfo {
        number: 348,
        rgb: [237, 234, 234],
        name: "Lily white",
    },
    BrickColorInfo {
        number: 349,
        rgb: [233, 218, 218],
        name: "Seashell",
    },
    BrickColorInfo {
        number: 350,
        rgb: [136, 62, 62],
        name: "Burgundy",
    },
    BrickColorInfo {
        number: 351,
        rgb: [188, 155, 93],
        name: "Cork",
    },
    BrickColorInfo {
        number: 352,
        rgb: [199, 172, 120],
        name: "Burlap",
    },
    BrickColorInfo {
        number: 353,
        rgb: [202, 191, 163],
        name: "Beige",
    },
    BrickColorInfo {
        number: 354,
        rgb: [187, 179, 178],
        name: "Oyster",
    },
    BrickColorInfo {
        number: 355,
        rgb: [108, 88, 75],
        name: "Pine Cone",
    },
    BrickColorInfo {
        number: 356,
        rgb: [160, 132, 79],
        name: "Fawn brown",
    },
    BrickColorInfo {
        number: 357,
        rgb: [149, 137, 136],
        name: "Hurricane grey",
    },
    BrickColorInfo {
        number: 358,
        rgb: [171, 168, 158],
        name: "Cloudy grey",
    },
    BrickColorInfo {
        number: 359,
        rgb: [175, 148, 131],
        name: "Linen",
    },
    BrickColorInfo {
        number: 360,
        rgb: [150, 103, 102],
        name: "Copper",
    },
    BrickColorInfo {
        number: 361,
        rgb: [86, 66, 54],
        name: "Dirt brown",
    },
    BrickColorInfo {
        number: 362,
        rgb: [126, 104, 63],
        name: "Bronze",
    },
    BrickColorInfo {
        number: 363,
        rgb: [105, 102, 92],
        name: "Flint",
    },
    BrickColorInfo {
        number: 364,
        rgb: [90, 76, 66],
        name: "Dark taupe",
    },
    BrickColorInfo {
        number: 365,
        rgb: [106, 57, 9],
        name: "Burnt Sienna",
    },
    BrickColorInfo {
        number: 1001,
        rgb: [248, 248, 248],
        name: "Institutional white",
    },
    BrickColorInfo {
        number: 1002,
        rgb: [205, 205, 205],
        name: "Mid gray",
    },
    BrickColorInfo {
        number: 1003,
        rgb: [17, 17, 17],
        name: "Really black",
    },
    BrickColorInfo {
        number: 1004,
        rgb: [255, 0, 0],
        name: "Really red",
    },
    BrickColorInfo {
        number: 1005,
        rgb: [255, 175, 0],
        name: "Deep orange",
    },
    BrickColorInfo {
        number: 1006,
        rgb: [180, 128, 255],
        name: "Alder",
    },
    BrickColorInfo {
        number: 1007,
        rgb: [163, 75, 75],
        name: "Dusty Rose",
    },
    BrickColorInfo {
        number: 1008,
        rgb: [193, 190, 66],
        name: "Olive",
    },
    BrickColorInfo {
        number: 1009,
        rgb: [255, 255, 0],
        name: "New Yeller",
    },
    BrickColorInfo {
        number: 1010,
        rgb: [0, 0, 255],
        name: "Really blue",
    },
    BrickColorInfo {
        number: 1011,
        rgb: [0, 32, 96],
        name: "Navy blue",
    },
    BrickColorInfo {
        number: 1012,
        rgb: [33, 84, 185],
        name: "Deep blue",
    },
    BrickColorInfo {
        number: 1013,
        rgb: [4, 175, 236],
        name: "Cyan",
    },
    BrickColorInfo {
        number: 1014,
        rgb: [170, 85, 0],
        name: "CGA brown",
    },
    BrickColorInfo {
        number: 1015,
        rgb: [170, 0, 170],
        name: "Magenta",
    },
    BrickColorInfo {
        number: 1016,
        rgb: [255, 102, 204],
        name: "Pink",
    },
    BrickColorInfo {
        number: 1017,
        rgb: [255, 175, 0],
        name: "Deep orange",
    },
    BrickColorInfo {
        number: 1018,
        rgb: [18, 238, 212],
        name: "Teal",
    },
    BrickColorInfo {
        number: 1019,
        rgb: [0, 255, 255],
        name: "Toothpaste",
    },
    BrickColorInfo {
        number: 1020,
        rgb: [0, 255, 0],
        name: "Lime green",
    },
    BrickColorInfo {
        number: 1021,
        rgb: [58, 125, 21],
        name: "Camo",
    },
    BrickColorInfo {
        number: 1022,
        rgb: [127, 142, 100],
        name: "Grime",
    },
    BrickColorInfo {
        number: 1023,
        rgb: [140, 91, 159],
        name: "Lavender",
    },
    BrickColorInfo {
        number: 1024,
        rgb: [175, 221, 255],
        name: "Pastel light blue",
    },
    BrickColorInfo {
        number: 1025,
        rgb: [255, 201, 201],
        name: "Pastel orange",
    },
    BrickColorInfo {
        number: 1026,
        rgb: [177, 167, 255],
        name: "Pastel violet",
    },
    BrickColorInfo {
        number: 1027,
        rgb: [159, 243, 233],
        name: "Pastel blue-green",
    },
    BrickColorInfo {
        number: 1028,
        rgb: [204, 255, 204],
        name: "Pastel green",
    },
    BrickColorInfo {
        number: 1029,
        rgb: [255, 255, 204],
        name: "Pastel yellow",
    },
    BrickColorInfo {
        number: 1030,
        rgb: [255, 204, 153],
        name: "Pastel brown",
    },
    BrickColorInfo {
        number: 1031,
        rgb: [98, 37, 209],
        name: "Royal purple",
    },
    BrickColorInfo {
        number: 1032,
        rgb: [255, 0, 191],
        name: "Hot pink",
    },
];

pub const PALETTE: [u16; 128] = [
    141, 301, 107, 26, 1012, 303, 1011, 304, 28, 1018, 302, 305, 306, 307, 308, 1021, 309, 310,
    1019, 135, 102, 23, 1010, 312, 313, 37, 1022, 1020, 1027, 311, 315, 1023, 1031, 316, 151, 317,
    318, 319, 1024, 314, 1013, 1006, 321, 322, 104, 1008, 119, 323, 324, 325, 320, 11, 1026, 1016,
    1032, 1015, 327, 1017, 1009, 29, 328, 1028, 208, 45, 329, 330, 331, 1004, 21, 332, 333, 24,
    334, 226, 1029, 335, 336, 342, 343, 338, 1007, 339, 133, 106, 340, 341, 1001, 1, 9, 1025, 337,
    344, 345, 1014, 105, 346, 347, 348, 349, 1030, 125, 101, 350, 192, 351, 352, 353, 354, 1002, 5,
    18, 217, 355, 356, 153, 357, 358, 359, 360, 38, 361, 362, 199, 194, 363, 364, 365, 1003,
];

pub const RENDERING_PALETTE: [u16; 128] = [
    141, 301, 107, 302, 26, 1012, 303, 1011, 304, 28, 1018, 135, 305, 306, 307, 308, 1021, 309,
    310, 1019, 311, 102, 23, 1010, 312, 313, 37, 1022, 1020, 1027, 314, 315, 1023, 1031, 316, 151,
    317, 318, 319, 320, 1024, 1013, 1006, 321, 322, 104, 1008, 119, 323, 324, 325, 11, 1026, 1016,
    1032, 1015, 327, 1017, 1009, 29, 328, 1028, 208, 45, 329, 330, 331, 1004, 21, 332, 333, 24,
    334, 226, 1029, 335, 336, 1025, 337, 338, 1007, 339, 133, 106, 40, 341, 1001, 1, 9, 342, 343,
    344, 345, 1014, 105, 346, 347, 348, 349, 1030, 125, 101, 350, 192, 351, 352, 353, 354, 1002, 5,
    18, 217, 355, 356, 153, 357, 358, 359, 360, 38, 361, 362, 199, 194, 363, 364, 365, 1003,
];

pub const DEFAULT: u16 = 194;
pub const WHITE: u16 = 1;
pub const GRAY: u16 = 194;
pub const DARK_GRAY: u16 = 199;
pub const BLACK: u16 = 26;
pub const RED: u16 = 21;
pub const YELLOW: u16 = 24;
pub const GREEN: u16 = 28;
pub const BASEPLATE_GREEN: u16 = 37;
pub const BLUE: u16 = 23;
