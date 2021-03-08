let toSend = null;

let mainStrength = document.getElementById("strength");

let from = document.getElementById("fromStrength");
let to = document.getElementById("toStrength");
let time = document.getElementById("transitionTime");
let interpolation = document.getElementById("interpolation");
let interpolationExtras = document.getElementById("interpolationExtras");

let day = document.getElementById("weekday");
let dayTime = document.getElementById("dayTime");
let dayOption = document.getElementById("optionTime");

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
    await fetch("/set-day-time", {
        method: 'POST',
        headers: {
            'content-type': 'application/json'
        },
        redirect: 'error',
        body: JSON.stringify({ day: day, time: time })
    });
    await getAndApplyState();
}
function getAndSendDayTime() {
    if (dayOption.value === "some") {
        sendDayTime(day.value, dayTime.value);
    } else {
        sendDayTime(day.value, null);
    }

}
function getAndSetTransition(action) {
    if (time.value !== "" && interpolation.value !== null) {
        fetch(`/transition?action=${action}`, {
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
    interpolationExtras.style.display = (interpolation.value.endsWith("-extra")) ? "initial" : "none";
}
function checkDailySchedulerOption() {
    dayTime.style.display = (dayOption.value === "some") ? "initial" : "none";
}

async function getAndApplyState() {
    let response = await (await fetch("/get-state")).json();
    console.log(response);

    mainStrength.value = response.strength;
    for (const day in response.days) {
        const time = response.days[day];
        const element = document.getElementById(day);

        if (element !== null) {
            element.innerHTML = (time === null) ? `No time set.` : `Time set at ${time}`;
        }
    }

}

async function load() {
    // let response = await (await fetch("/get-strength")).text();
    // mainStrength.value = Number(response) / 255;
    await getAndApplyState();
}

load();
checkTransitionExtras();
checkDailySchedulerOption();
