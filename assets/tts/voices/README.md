# echOS TTS Voices

This directory carries local voice-profile assets for the shell-owned TTS path.

Current upstream source:
- `espeak-ng` voice profiles from `espeak-ng-data/voices/!v`

Vendored files:
- `espeak-ng/Alex`
- `espeak-ng/Alicia`
- `espeak-ng/Gene`
- `espeak-ng/COPYING`

Why these files are vendored:
- echOS accessibility speech must keep working without host filesystem dependencies
- build-time inclusion keeps provenance explicit and runtime deterministic

License posture:
- these vendored voice profiles are carried under the upstream `COPYING` file in the same directory
- echOS repository license is AGPL, so GPL-compatible voice data is acceptable for this path
