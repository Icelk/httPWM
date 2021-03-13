!> cache dynamic
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

let schedulerKind = document.getElementById("schedulerKind");
let schedulerDate = document.getElementById("schedulerDate");
let schedulerWeekday = document.getElementById("schedulerWeekday");
let schedulerTime = document.getElementById("schedulerTime");
let schedulerName = document.getElementById("schedulerName");
let schedulerDescription = document.getElementById("schedulerDescription");

window.setInterval(() => {
    if (toSend !== null) {
        sendSet(toSend);
        toSend = null;
    }
}, 25);


let currentNotification = null;
const notificationInfo = "var(--bg-second)";
const notificationError = "#8c1c2e";
const notificationTransform = "translateX(20em)";
function sendNotification(message, color) {
    tryClearNotification();

    let element = document.createElement("span");
    element.classList.add("notification");
    element.style.backgroundColor = color;
    element.innerHTML = message;
    element.addEventListener("click", () => tryClearNotification());

    document.body.appendChild(element);

    setTimeout(() => { element.style.transform = "none"; }, 20);

    let timeout1 = setTimeout(() => { currentNotification.element.style.transform = notificationTransform }, 5000);
    let timeout2 = setTimeout(() => { tryClearNotification() }, 5500);

    currentNotification = { element: element, timeout1: timeout1, timeout2: timeout2 };
}
function tryClearNotification() {
    if (currentNotification !== null) {
        let element = currentNotification.element;

        element.style.transform = notificationTransform;
        setTimeout(() => { document.body.removeChild(element) }, 500);
        clearTimeout(currentNotification.timeout1);
        clearTimeout(currentNotification.timeout2);

        currentNotification = null;
    }
}


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
function getTransition() {
    return {
        from: Number(from.value),
        to: Number(to.value),
        time: Number(time.value),
        interpolation: interpolation.value,
        extras: [interpolationExtras.value]
    };
}
function getAndSetTransition(action) {
    if (time.value !== "" && interpolation.value !== null) {
        fetch(`/transition?action=${action}`, {
            method: 'POST',
            headers: {
                'content-type': 'application/json',
            },
            redirect: 'error',
            body: JSON.stringify(getTransition())
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

async function overrideSchedulerList() {
    alert("Called unimplemented function!");
}
function checkSchedulerAddExtras() {
    let { date, day } = getSchedulerExtras();

    schedulerDate.style.display = date ? "" : "none";
    schedulerWeekday.style.display = day ? "" : "none";
}
function getSchedulerExtras() {
    let kind = schedulerKind.value;
    let date = false;
    let day = false;

    if (kind === "at") {
        date = true;
    } else if (kind === "every-week") {
        day = true;
    }
    return { date: date, day: day };
}
async function getAndAddScheduler() {
    let kind = schedulerKind.value;
    let time = schedulerTime.value;
    let name = schedulerName.value;
    let description = schedulerDescription.value;
    // let extras = {};
    let extras = [];
    let { date: send_date, day: send_day } = getSchedulerExtras();
    if (send_date) {
        // extras.date = schedulerDate.value;
        extras.push(schedulerDate.value);
    }
    if (send_day) {
        // extras.day = schedulerWeekday.value;
        extras.push(schedulerWeekday.value);
    }

    const body = { kind: kind, time: time, name: name, description: description, extras: extras, transition: getTransition() };

    console.log(body);

    await fetch("/add-scheduler", {
        method: 'POST',
        headers: {
            "content-type": "application/json",
        },
        redirect: 'error',
        body: JSON.stringify(body),
    })
}

async function load() {
    await getAndApplyState();
}

load();
checkTransitionExtras();
checkDailySchedulerOption();
checkSchedulerAddExtras();
