use std;

#[derive(Debug, Copy, Clone)]
pub struct IntVec2 {
    pub x: i32,
    pub y: i32,
}

impl std::convert::From<Vec2> for IntVec2 {
    fn from(vec: Vec2) -> IntVec2 {
        IntVec2 {
            x: vec.x as i32,
            y: vec.y as i32,
        }
    }
}

impl std::ops::Add<IntVec2> for IntVec2 {
    type Output = IntVec2;

    fn add(self, other: IntVec2) -> IntVec2 {
        IntVec2 {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl std::ops::Sub<IntVec2> for IntVec2 {
    type Output = IntVec2;

    fn sub(self, other: IntVec2) -> IntVec2 {
        IntVec2 {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl std::ops::Mul<i32> for IntVec2 {
    type Output = IntVec2;

    fn mul(self, scalar: i32) -> IntVec2 {
        IntVec2 {
            x: self.x * scalar,
            y: self.y * scalar,
        }
    }
}

impl std::ops::Mul<IntVec2> for i32 {
    type Output = IntVec2;

    fn mul(self, vec: IntVec2) -> IntVec2 {
        vec * self
    }
}

impl std::ops::Div<i32> for IntVec2 {
    type Output = IntVec2;

    fn div(self, scalar: i32) -> IntVec2 {
        IntVec2 {
            x: self.x / scalar,
            y: self.y / scalar,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub fn round(self) -> Vec2 {
        Vec2 {
            x: self.x.round(),
            y: self.y.round(),
        }
    }
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}

impl std::convert::From<IntVec2> for Vec2 {
    fn from(vec: IntVec2) -> Vec2 {
        Vec2 {
            x: vec.x as f32,
            y: vec.y as f32,
        }
    }
}

impl std::ops::Add<Vec2> for Vec2 {
    type Output = Vec2;

    fn add(self, other: Vec2) -> Vec2 {
        Vec2 {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl std::ops::Sub<Vec2> for Vec2 {
    type Output = Vec2;

    fn sub(self, other: Vec2) -> Vec2 {
        Vec2 {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl std::ops::Mul<f32> for Vec2 {
    type Output = Vec2;

    fn mul(self, scalar: f32) -> Vec2 {
        Vec2 {
            x: self.x * scalar,
            y: self.y * scalar,
        }
    }
}

impl std::ops::Mul<Vec2> for f32 {
    type Output = Vec2;

    fn mul(self, vec: Vec2) -> Vec2 {
        vec * self
    }
}

impl std::ops::Div<f32> for Vec2 {
    type Output = Vec2;

    fn div(self, scalar: f32) -> Vec2 {
        Vec2 {
            x: self.x / scalar,
            y: self.y / scalar,
        }
    }
}
