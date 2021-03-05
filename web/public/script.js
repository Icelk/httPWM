let toSend = null;

let mainStrength = document.getElementById("strength");

let day = document.getElementById("weekday");
let dayTime = document.getElementById("dayTime");

let from = document.getElementById("fromStrength");
let to = document.getElementById("toStrength");
let time = document.getElementById("transitionTime");
let interpolation = document.getElementById("interpolation");
let interpolationExtras = document.getElementById("interpolationExtras");

window.setInterval(() => {
    if (toSend !== null) {
        sendSet(toSend);
        toSend = null;
    }
}, 25);

async function sendSet(strength) {
    fetch(`/set-strength?strength=${strength}`).await;
}
// Day must exist, can be 'mon', 'tue', etc.
// Time can be null or "HH:MM:SS" format.
async function sendDayTime(day, time) {
    fetch("/set-day-time", {
        method: 'POST',
        headers: {
            'content-type': 'application/json'
        },
        redirect: 'error',
        body: JSON.stringify({ day: day, time: time })
    }).await
}
function getAndSendDayTime() {
    if (day.value !== null) {
        sendDayTime(day.value, dayTime.value);
    }
}
function getAndSetTransition() {
    if (time.value !== "" && interpolation.value !== null) {
        fetch("/set-transition", {
            method: 'POST',
            headers: {
                'content-type': 'application/json',
            },
            redirect: 'error',
            body: JSON.stringify({ from: Number(from.value), to: Number(to.value), time: Number(time.value), interpolation: interpolation.value, extras: [interpolationExtras.value] })
        })
    }
}
function checkTransitionExtras() {
    if (interpolation.value.endsWith("-extra")) {
        interpolationExtras.style.display = "initial";
        if (interpolation.value.startsWith("linear")) {
            interpolationExtras.placeholder = "Fade out duration, multiplier of 'time'";
        }
    } else {
        interpolationExtras.style.display = "none";
    }
}

async function load() {
    let response = await (await fetch("/get-strength")).text();
    mainStrength.value = Number(response) / 255;
}

load();
