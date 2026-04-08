# 🎮 Super Mario 4 — Enhanced Edition — Quick Start Guide

## New & Improved Features! ✨

### Core Gameplay Improvements

**Better Jump Control**
- Longer jump buffer (12 frames instead of 10) = more forgiving timing
- Longer coyote time = jump off ledges up to 0.2 seconds after leaving platform
- Variable jump height = hold Space for high jump, tap for short hop

**Responsive Movement**
- Faster acceleration on ground (1.8 vs 1.6)
- Tighter air control for mid-air maneuvers
- Faster run speed (7.0 units)
- Wall sliding: collide with a wall while falling to slide down smoothly (new!)

**Better Feedback**
- **Screen shake** on:
  - Jumping (small shake for responsiveness)
  - Stomping enemies (bigger shake for impact)
  - Kicking shells (intense shake for satisfaction)
- Faster animations (5 frames vs 6)
- Enhanced particle effects on impacts

**Faster Fire Rate**
- Fire cooldown reduced from 22 to 18 frames
- Shoot more often with fireflower power-up

### Mobile Controls (Much Better!)
- Touch buttons are now **70px** (bigger, easier to tap!)
- Higher opacity (0.50 vs 0.45) for better visibility
- Darker backgrounds with white borders
- Clear labels: "JUMP", "RUN" instead of single letters
- Better spacing for comfortable thumb control

---

## How to Play

### Keyboard Controls
| Key | Action |
|-----|--------|
| **← →** or **A D** | Move left/right |
| **Space** / **↑** / **W** | Jump |
| **Shift** / **Z** | Run fast |
| **X** / **F** | Shoot fire (with 🌸) |
| **P** | Pause/unpause |

### Mobile Controls
- **Left/Right buttons**: Move
- **JUMP button** (bottom-right): Jump (hold for higher jump!)
- **RUN button** (bottom-right): Hold to run fast
- **🔥 button** (top-right): Shoot fireballs (if you have them)

### Gameplay Tips

**Stomping Enemies**
1. Jump on enemy heads from above
2. Get bounced up automatically
3. Chain stomps = higher combo multiplier = more points!

**Wall Sliding** (NEW!)
1. Jump at a wall
2. Collide with it while falling
3. You'll slide down smoothly instead of bouncing off
4. Perfect for finding secret passages!

**Collecting Power-ups**
- 🍄 **Mushroom**: Grow big (and tougher!)
- 🌸 **Fire Flower**: Shoot fireballs (becomes big too)
- ⭐ **Star**: Temporary invincibility + instant enemy kills

**Building Combos**
- Stomp multiple enemies in quick succession
- Each hit increases combo counter
- Higher combo = more points per enemy
- Combo resets if you wait too long

**Reaching the Goal**
- Get to the flag at the end of the level
- Higher position on the pole = more bonus points
- Bounce on enemy heads to reach higher platforms!

---

## What Changed (For Developers)

### Physics Constants
```javascript
GRAVITY: 0.56 (was 0.58)
JUMP_FORCE: -14.2 (was -13.8)
RUN_SPEED: 7.0 (was 6.8)
FRICTION: 0.82 (was 0.80)
MAX_FALL: 17 (was 18)
```

### New Player Properties
```javascript
player.onWall      // Is player currently on a wall?
player.wallSlideDir // Which direction? -1 (left) or 1 (right)
player.dashCooldown // Ready for dash? (infrastructure prepared)
player.dashDir      // Which direction to dash? (infrastructure prepared)
```

### Camera Shake System
```javascript
cameraShakeX        // Horizontal shake offset
cameraShakeY        // Vertical shake offset
cameraShakeIntensity // Current shake strength (decays at 0.92/frame)
```

### Improved Mechanics
- Jump buffer: 10 → 12 frames
- Coyote time: 10 → 12 frames
- Fire cooldown: 22 → 18 frames
- Stomp bounce: -10 → -11 units
- Ground acceleration: 1.6 → 1.8

---

## Performance Notes

✅ **Smooth 60 FPS** on modern browsers  
✅ **Mobile-friendly** with optimized touch input  
✅ **Responsive feedback** via camera shake (minimal performance impact)  
✅ **Progressive enhancement** — game works on all devices  

---

## Known Features

✅ 3 full levels with different themes  
✅ 4 enemy types (Goomba, Koopa, Piranha, Shells)  
✅ Combo system with visual feedback  
✅ Star invincibility power-up  
✅ Fire flower shooting mechanic  
✅ Score and life tracking  
✅ Pause/resume functionality  
✅ Touch controls for mobile  
✅ Parallax scrolling backgrounds  
✅ Smooth camera following  

---

## Testing Checklist

When you play, try these:

- [ ] Jump off a ledge and jump again mid-air (coyote time)
- [ ] Press jump JUST after walking off platform (jump buffer)
- [ ] Hold jump for max height, tap for short hop
- [ ] Jump against a wall and slide down smoothly
- [ ] Stomp 5+ enemies in a row (watch the combo text!)
- [ ] Get screen shake feedback on impacts
- [ ] Use fireflower to break bricks from a distance
- [ ] Reach the flag for the bonus points
- [ ] Try mobile controls on a touch device

---

## Future Possibilities 🚀

The infrastructure is ready for:
- Wall jump mechanic (jump off walls while sliding)
- Dash move (left/right quick burst)
- Double jump for specific power-ups
- Advanced particle effects
- More enemy types
- New level themes

---

**Enjoy the enhanced Mario experience! 🎮⭐**

Updated: 2026-04-08 | Version: Enhanced Edition
