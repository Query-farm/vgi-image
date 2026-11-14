"""Presentation helpers; analytical transformations remain visible SQL."""

from pathlib import Path

import matplotlib.pyplot as plt
import seaborn as sns


def theme():
    """Use consistent readable chart defaults."""
    sns.set_theme(style="whitegrid", context="notebook", font_scale=1.08)
    plt.rcParams.update(
        {
            "figure.dpi": 130,
            "savefig.dpi": 150,
            "text.parse_math": False,
            "axes.spines.top": False,
            "axes.spines.right": False,
        }
    )


def finish(fig, name, title, subtitle):
    """Reserve title and source space, export a local review chart, and display."""
    fig.suptitle(title, x=0.10, y=0.98, ha="left", fontsize=17, fontweight="bold", color="#243e32")
    fig.text(0.10, 0.922, subtitle, ha="left", fontsize=11, color="#526b5b")
    fig.text(
        0.10,
        0.025,
        "Source: unchanged synthetic image fixtures from vgi-image · no real photographs or observed GPS\n"
        "Real vgi-image v0.4.0 through Haybarn / VGI · candidate similarity does not establish duplicate identity",
        fontsize=9,
        color="#526b5b",
    )
    fig.tight_layout(rect=(0.02, 0.105, 0.98, 0.85))
    folder = Path("validation/charts")
    folder.mkdir(parents=True, exist_ok=True)
    fig.savefig(folder / f"{name}.png", bbox_inches="tight")
    plt.show()
