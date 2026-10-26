{
  "patcher": {
    "fileversion": 1,
    "appversion": {
      "major": 9,
      "minor": 2,
      "revision": 0,
      "architecture": "arm64",
      "modernui": 1
    },
    "classnamespace": "box",
    "rect": [
      50.0,
      50.0,
      1000.0,
      740.0
    ],
    "default_fontsize": 13.0,
    "default_fontface": 0,
    "default_fontname": "Arial",
    "boxes": [
      {
        "box": {
          "id": "title",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25,
            20,
            930,
            30
          ],
          "text": "4. Buffer edit exclusion \u2014 open the Max console"
        }
      },
      {
        "box": {
          "id": "intro",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25,
            60,
            930,
            50
          ],
          "text": "This read-only probe compares SDK pointers and attempts shared claims during an edit. No sample data is read or written. Each probe briefly pauses the UI (about 450 ms)."
        }
      },
      {
        "box": {
          "id": "buffer",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 2,
          "patching_rect": [
            25,
            135,
            290,
            24
          ],
          "text": "buffer~ #0-buffer-probe 1000 2",
          "outlettype": [
            "float",
            "bang"
          ]
        }
      },
      {
        "box": {
          "id": "dedicated",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            340,
            135,
            610,
            40
          ],
          "text": "Dedicated two-channel buffer, 1000 ms. Keep it unchanged during a probe."
        }
      },
      {
        "box": {
          "id": "probe",
          "maxclass": "message",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            25,
            215,
            65,
            24
          ],
          "text": "probe",
          "outlettype": [
            ""
          ]
        }
      },
      {
        "box": {
          "id": "external",
          "maxclass": "newobj",
          "numinlets": 1,
          "numoutlets": 1,
          "patching_rect": [
            25,
            270,
            355,
            24
          ],
          "text": "median.validate.buffer~ #0-buffer-probe",
          "outlettype": [
            "signal"
          ]
        }
      },
      {
        "box": {
          "id": "snapshot",
          "maxclass": "newobj",
          "numinlets": 2,
          "numoutlets": 1,
          "patching_rect": [
            25,
            330,
            130,
            24
          ],
          "text": "snapshot~ 100",
          "outlettype": [
            "float"
          ]
        }
      },
      {
        "box": {
          "id": "zero",
          "maxclass": "flonum",
          "numinlets": 1,
          "numoutlets": 2,
          "patching_rect": [
            25,
            380,
            80,
            24
          ],
          "outlettype": [
            "",
            "bang"
          ]
        }
      },
      {
        "box": {
          "id": "dac",
          "maxclass": "ezdac~",
          "numinlets": 2,
          "numoutlets": 0,
          "patching_rect": [
            430,
            275,
            45,
            45
          ]
        }
      },
      {
        "box": {
          "id": "dacnote",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            490,
            275,
            430,
            40
          ],
          "text": "DSP toggle only; no signal connected to speakers."
        }
      },
      {
        "box": {
          "id": "off",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25,
            450,
            930,
            45
          ],
          "text": "1. DSP OFF: click probe. Expect main_exclusion=PASS, pointer_identity=PASS, audio_exclusion=INCONCLUSIVE (no audio callbacks)."
        }
      },
      {
        "box": {
          "id": "on",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25,
            510,
            930,
            55
          ],
          "text": "2. Enable DSP with ezdac~, then click probe. Expect audio_exclusion=PASS: successful claims before/after, zero successful claims during the edit, and at least one refusal during it."
        }
      },
      {
        "box": {
          "id": "repeat",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25,
            580,
            930,
            55
          ],
          "text": "3. Repeat with Overdrive on/off. Keep Scheduler in Audio Interrupt off for the first run. Copy all four console lines per probe and note DSP, Overdrive, and Scheduler in Audio Interrupt settings."
        }
      },
      {
        "box": {
          "id": "limits",
          "maxclass": "comment",
          "numinlets": 1,
          "numoutlets": 0,
          "patching_rect": [
            25,
            650,
            930,
            55
          ],
          "text": "If any check says FAIL, save the output and stop. INCONCLUSIVE with DSP on means we need to inspect callback activity. Pointer identity alone does not establish writable-slice safety or prove all concurrency cases."
        }
      }
    ],
    "lines": [
      {
        "patchline": {
          "source": [
            "probe",
            0
          ],
          "destination": [
            "external",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "external",
            0
          ],
          "destination": [
            "snapshot",
            0
          ]
        }
      },
      {
        "patchline": {
          "source": [
            "snapshot",
            0
          ],
          "destination": [
            "zero",
            0
          ]
        }
      }
    ]
  }
}
