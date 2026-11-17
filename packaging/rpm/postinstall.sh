# Apply the Corne udev rule (70-corne-hud.rules) without a replug.
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules || :
    udevadm trigger --subsystem-match=hidraw --subsystem-match=input || :
fi
