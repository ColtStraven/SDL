# Record One Controlled Flight

Time needed: about 15 minutes. You fly normally. SDL does the rest.

## Which computer

Run the recorder on **the computer that runs War Thunder**. That is this PC. The recorder reads `127.0.0.1:8111`, which exists only on the game's computer. GeForce NOW will not work, because the game runs on a remote computer.

The recorder only reads. It sends nothing to the game, draws nothing on screen, and does not touch Strike Master.

## Steps

1. **Start the recorder.** Double-click `Record Flight.bat` in `static57-sdl\sdl-replay-decoder`. A black window opens. Leave it open. It can start before the game.
2. **Start a Custom Battle** with one aircraft you want to study. Turn on `Allow web UI` if the battle settings show that option. Use a normal battle that saves a replay, the way your existing replays were made.
3. **Fly the short profile below.**
4. **Finish the battle** and return to the hangar, so the game saves the replay.
5. **Stop the recorder.** Click its window, type `q`, press Enter.
6. **Tell Claude the flight is done.** Claude runs the comparison.

The recorder's "flight readings" number should climb while you fly. If it stays at 0, you are not in the aircraft yet, or the game's web interface is off.

## Flight profile

Each step can be done roughly. Order does not matter, and a missed step only leaves a gap.

| Do this | About | Tests |
|---|---|---|
| Take off and raise the gear | 1 min | Gear, flaps, throttle |
| **Roll hard left, right, left, right** | 15 s | Time alignment |
| Fly level on four headings 90° apart | 2 min each | Compass, map position, wind |
| Climb steadily, then dive | 1 min each | Altitude, vertical speed |
| One full roll, then one full 360° turn | 1 min | Attitude, load factor |
| Throttle: idle, half, full, afterburner if fitted, 10 s each | 1 min | Throttle, RPM, fuel |
| Flaps out and in, airbrake out and in, at safe speed | 30 s | Flaps, airbrake |

Do not pause the game. A pause breaks the time alignment for the flight.

## Optional marks

Pressing Enter in the recorder window adds a time mark. You can type a note first. You never need a mark, because SDL aligns time from the cockpit clock and the aircraft's motion.

## After the flight

From the SDL folder, with the capture folder name the recorder printed:

```bash
static57-sdl/sdl-replay-decoder/target/release/sdl-replay-decoder telemetry-validate telemetry/<folder> "C:/Users/Shadow/AppData/Local/WarThunder/Replays"
```

The report is written to `research/<replay name>/validation-report.md`, and the full figures to `validation-data.json`.

The extended test sequences, including weapon shots, are in [CONTROLLED_TEST_PROCEDURE.md](CONTROLLED_TEST_PROCEDURE.md).
