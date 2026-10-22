//! Short, memorable device names. A name is generated once and stored as identity content.
use uuid::Uuid;

const MOODS: &[&str] = &[
    "悠闲",
    "机灵",
    "打盹",
    "迷路",
    "好奇",
    "横行",
    "快乐",
    "安静",
    "勇敢",
    "害羞",
    "活泼",
    "温柔",
    "认真",
    "慵懒",
    "敏捷",
    "耐心",
    "热心",
    "自在",
    "顽皮",
    "神气",
    "从容",
    "专注",
    "潇洒",
    "腼腆",
    "乐观",
    "勤快",
    "聪明",
    "淡定",
    "优雅",
    "俏皮",
    "憨厚",
    "热情",
    "散步",
    "听雨",
    "追风",
    "看海",
    "望月",
    "晒太阳",
    "数星星",
    "做梦",
    "探险",
    "漫游",
    "踏浪",
    "兜风",
    "哼歌",
    "跳舞",
    "发呆",
    "伸懒腰",
];

// Insects and crustaceans only; keep familiar names that fit the compact receive page.
const CREATURES: &[&str] = &[
    "寄居蟹",
    "招潮蟹",
    "龙虾",
    "小龙虾",
    "皮皮虾",
    "河虾",
    "对虾",
    "磷虾",
    "帝王蟹",
    "雪蟹",
    "梭子蟹",
    "面包蟹",
    "青蟹",
    "沙蟹",
    "椰子蟹",
    "丰年虾",
    "螳螂",
    "瓢虫",
    "萤火虫",
    "蝴蝶",
    "蜻蜓",
    "豆娘",
    "蜜蜂",
    "熊蜂",
    "蚂蚁",
    "蟋蟀",
    "蝈蝈",
    "蚱蜢",
    "蝉",
    "金龟子",
    "独角仙",
    "锹甲",
    "天牛",
    "虎甲",
    "吉丁虫",
    "竹节虫",
    "凤蝶",
    "枯叶蝶",
    "蚕蛾",
    "螽斯",
];

pub fn generate(previous: &str) -> String {
    let count = MOODS.len() * CREATURES.len();
    let start = (Uuid::new_v4().as_u128() % count as u128) as usize;
    // Walk past the previous name, so “换个名字” always produces a different name.
    for offset in 0..count {
        let index = (start + offset) % count;
        let name = format!(
            "{}的{}",
            MOODS[index / CREATURES.len()],
            CREATURES[index % CREATURES.len()]
        );
        if name != previous {
            return name;
        }
    }
    unreachable!("the name pool contains more than one distinct name")
}
